#!/usr/bin/env python3
"""
Fix historical viewer observations and stream history corrupted by stream resets.

Before the stream reset detection feature, when a Twitch stream reset (OBS crash,
internet blip), the new started_at was recorded as-is. This caused:
  1. viewer_observations with incorrect stream_age_min (~0 instead of real age)
  2. viewer_observations with wrong stream_started_at (the reset time, not original)
  3. duplicate stream_history entries for what was logically one stream

This script detects reset pairs and merges them: observations from the reset stream
get their stream_started_at and stream_age_min corrected to reflect the original
stream, and duplicate stream_history entries are removed.

Detection criteria: two streams for the same broadcaster where the observation gap
(last obs of stream A to first obs of stream B) is less than MAX_OBS_GAP_MIN.
This is deliberately conservative — it's better to miss an edge case than to
incorrectly merge two genuinely separate streams.

Usage:
    python3 scripts/fix-stream-resets.py [--dry-run] [--db PATH]
"""

import argparse
import shutil
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_DB = Path.home() / ".config" / "twitch-tray" / "data.db"

# Maximum gap between last observation of stream A and first observation of stream B
# to consider them the same stream. The poll interval is 60s, so a gap of <=5 min
# means the stream was offline for at most ~5 polls — consistent with a brief reset.
MAX_OBS_GAP_MIN = 5


def ts_to_str(ts: int) -> str:
    return datetime.fromtimestamp(ts, tz=timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")


def find_reset_pairs(conn: sqlite3.Connection) -> list[dict]:
    """Find pairs of streams that are likely resets of each other."""
    cursor = conn.execute("""
        WITH stream_windows AS (
            SELECT broadcaster_id, stream_started_at,
                   MIN(observed_at) AS first_obs,
                   MAX(observed_at) AS last_obs,
                   COUNT(*) AS obs_count
            FROM viewer_observations
            WHERE stream_started_at > 0
            GROUP BY broadcaster_id, stream_started_at
        )
        SELECT a.broadcaster_id,
               a.stream_started_at AS orig_start,
               b.stream_started_at AS reset_start,
               a.last_obs AS orig_last_obs,
               b.first_obs AS reset_first_obs,
               a.obs_count AS orig_obs,
               b.obs_count AS reset_obs
        FROM stream_windows a
        JOIN stream_windows b ON a.broadcaster_id = b.broadcaster_id
            AND b.stream_started_at > a.stream_started_at
            AND (b.first_obs - a.last_obs) <= ?1 * 60
        ORDER BY a.broadcaster_id, a.stream_started_at
    """, (MAX_OBS_GAP_MIN,))

    pairs = []
    for row in cursor:
        pairs.append({
            "broadcaster_id": row[0],
            "orig_start": row[1],
            "reset_start": row[2],
            "orig_last_obs": row[3],
            "reset_first_obs": row[4],
            "obs_gap_min": (row[4] - row[3]) / 60,
            "orig_obs": row[5],
            "reset_obs": row[6],
        })
    return pairs


def resolve_chains(pairs: list[dict]) -> dict[tuple[int, int], int]:
    """Resolve transitive reset chains: if C resets B resets A, both map to A.

    Returns a mapping of (broadcaster_id, reset_start) -> canonical orig_start.
    """
    # Build a graph: for each (bid, reset_start) -> immediate orig_start
    immediate = {}
    for p in pairs:
        key = (p["broadcaster_id"], p["reset_start"])
        # If multiple pairs point to the same reset_start, keep the earliest orig
        if key not in immediate or p["orig_start"] < immediate[key]:
            immediate[key] = p["orig_start"]

    # Follow chains to find the canonical (earliest) original
    canonical = {}
    for key in immediate:
        bid = key[0]
        orig = immediate[key]
        # Walk back: is this orig itself a reset of something earlier?
        while (bid, orig) in immediate:
            orig = immediate[(bid, orig)]
        canonical[key] = orig

    return canonical


def get_broadcaster_name(conn: sqlite3.Connection, bid: int) -> str:
    row = conn.execute(
        "SELECT broadcaster_name FROM followed WHERE broadcaster_id = ?", (bid,)
    ).fetchone()
    return row[0] if row else f"(unknown:{bid})"


def fix_observations(
    conn: sqlite3.Connection,
    canonical: dict[tuple[int, int], int],
    dry_run: bool,
) -> int:
    """Fix stream_started_at and stream_age_min in viewer_observations."""
    total_fixed = 0

    for (bid, reset_start), orig_start in canonical.items():
        name = get_broadcaster_name(conn, bid)

        # Count affected rows
        count = conn.execute(
            "SELECT COUNT(*) FROM viewer_observations "
            "WHERE broadcaster_id = ? AND stream_started_at = ?",
            (bid, reset_start),
        ).fetchone()[0]

        if count == 0:
            continue

        obs_gap = (reset_start - orig_start) / 60
        print(
            f"  {name} (id={bid}): {count} observations, "
            f"reset {ts_to_str(reset_start)} -> orig {ts_to_str(orig_start)} "
            f"(stream was {obs_gap:.0f}min old at reset)"
        )

        if not dry_run:
            # Update stream_started_at and recalculate stream_age_min
            conn.execute("""
                UPDATE viewer_observations
                SET stream_started_at = ?1,
                    stream_age_min = (observed_at - ?1) / 60
                WHERE broadcaster_id = ?2 AND stream_started_at = ?3
            """, (orig_start, bid, reset_start))

        total_fixed += count

    return total_fixed


def fix_stream_history(
    conn: sqlite3.Connection,
    canonical: dict[tuple[int, int], int],
    dry_run: bool,
) -> int:
    """Remove duplicate stream_history entries created by resets."""
    total_removed = 0

    for (bid, reset_start), orig_start in canonical.items():
        # Check if the reset entry exists in stream_history
        exists = conn.execute(
            "SELECT 1 FROM stream_history WHERE user_id = ? AND started_at = ?",
            (bid, reset_start),
        ).fetchone()

        if not exists:
            continue

        # Also verify the original entry exists (it should)
        orig_exists = conn.execute(
            "SELECT 1 FROM stream_history WHERE user_id = ? AND started_at = ?",
            (bid, orig_start),
        ).fetchone()

        name = get_broadcaster_name(conn, bid)
        if orig_exists:
            print(
                f"  {name} (id={bid}): removing duplicate "
                f"started_at={ts_to_str(reset_start)} (original={ts_to_str(orig_start)})"
            )
            if not dry_run:
                conn.execute(
                    "DELETE FROM stream_history WHERE user_id = ? AND started_at = ?",
                    (bid, reset_start),
                )
            total_removed += 1
        else:
            # Original doesn't exist — update the reset entry to the original time
            print(
                f"  {name} (id={bid}): updating started_at "
                f"{ts_to_str(reset_start)} -> {ts_to_str(orig_start)}"
            )
            if not dry_run:
                conn.execute(
                    "UPDATE stream_history SET started_at = ? "
                    "WHERE user_id = ? AND started_at = ?",
                    (orig_start, bid, reset_start),
                )
            total_removed += 1

    return total_removed


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dry-run", action="store_true", help="Show what would change without modifying the database")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB, help=f"Path to data.db (default: {DEFAULT_DB})")
    args = parser.parse_args()

    db_path = args.db
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        sys.exit(1)

    if args.dry_run:
        print(f"DRY RUN — no changes will be made\n")
    else:
        # Back up database before making changes
        backup = db_path.with_suffix(".db.bak-reset-fix")
        print(f"Backing up database to {backup}")
        shutil.copy2(db_path, backup)
        print()

    conn = sqlite3.connect(str(db_path))

    # Step 1: Find reset pairs
    pairs = find_reset_pairs(conn)
    if not pairs:
        print("No stream resets detected. Nothing to fix.")
        conn.close()
        return

    # Step 2: Resolve chains
    canonical = resolve_chains(pairs)

    print(f"Found {len(canonical)} reset(s) across {len(set(k[0] for k in canonical))} broadcaster(s)\n")

    # Step 3: Fix observations
    print("Fixing viewer_observations:")
    obs_fixed = fix_observations(conn, canonical, args.dry_run)
    print(f"  Total: {obs_fixed} observations {'would be' if args.dry_run else ''} updated\n")

    # Step 4: Fix stream history
    print("Fixing stream_history:")
    hist_fixed = fix_stream_history(conn, canonical, args.dry_run)
    print(f"  Total: {hist_fixed} entries {'would be' if args.dry_run else ''} removed/updated\n")

    if not args.dry_run:
        conn.commit()
        print("Done. Changes committed.")
    else:
        print("Dry run complete. Re-run without --dry-run to apply changes.")

    conn.close()


if __name__ == "__main__":
    main()
