# TODO

This is the only file a human has edited.

## Bugs!


- free() corrupted unsorted chunks
- version when starting and tag version (used for creating builds in CI) are different. What is the idiomatic rust way of solving this? I want to set the version once. We can create a CI task for releasing if that's appropriate


## TODO:

- add general setting for how many followed and how many scheduled to show in main dropdown
- better schedule. Currently we only check the "first" 50. Instead, how about we check everyone once every 24hrs, with a max of 10 a minute, drip fed into the database. It's not going to change that much, and we can cache and cover everything. menu should also update schedule much fast once it's decoupled, we shouldn't see schduled streams for overnight when we wake up
- make sure we are detecting wake ups and instantly refreshing if we've been asleep. This should be magic if we schedule our refreshes on a timer?

- ask it about the security of the user's credential. How is it stored, can we take another crack at storing it in the keychain?
- get a better tray icon, this one looks too large comparatively
- better packaging, installed "properly" (arch aur?), starts at startup etc
- cron to periodically delete data older than N months from the database
- refactor DB usage so it handles logout / login. Maybe name the DB the logged in user's id?

- RUST: do we really need a makefile now? If we need an external build, is there something better than make?
- RUST: can we move cargo.toml etc to the root?

## Doing:


- inferred magic schedules
- audit our API usage and make sure we are abusing the API the minimum amount. We should find out live list once a minute and no more.
- refactor all places we are querying the api and deal with retries and auth / refresh keys centrally.
- general refactor of how refreshing is done to decouple data loading with reacting to data changing

## Done:

- send notification when a streamers category changes, showing old categoruy -> new category.
  - do desktop OS' support categories of notifications, so they can be configured by the OS?
- work backwards and get good test coverage. Think of something to add to claude.md about how it is allowed to add tests, but isn't allowed to edit or delete  tests unless it has a good reason?
- investigate the level of interactability we can support in notifications cross-platform
  - rewrite in rust
- show first 10 live and first 5 scheduled inline
- detect sleep-wake scenarios, and refresh the data.
  - we are getting 
2026-02-01T08:08:40.076830Z ERROR twitch_tray::app: Failed to get followed streams: API error 401 Unauthorized: {"error":"Unauthorized","status":401,"message":"Invalid OAuth token"}
    correctly detect wake and get a new token, or just detect 401s and get a new token
  - consider: if the sleep period wasn't very long, eg <10min, do we want send a notification for any changes in the sleep time?
- investigate: can it really not work out what categories a user follows? What options are therefor that?
- settings
- fixed "a" core dump, but not this specific one. We now know how to report core dumps to claude
running make run, after awhile, lots of wake and sleeps I'm sure.
```
corrupted double-linked list
make: *** [Makefile:26: run] Aborted (core dumped)
```
- fix windows build
- add retries to refreshing state. When a computer first wakes the network might not be restored. So retry until it's done. Have that as one function¸ so the decision on whether to show notifications is based on that. Otherwise you wake, fail to retry, retry a minute later, show updates for all changes since sleep.
  - actually, is there a websocket version of this api? That way we don't care about sleep and wake anymore, we care about socket connections?
- RUST: do we really need all of those dependencies? cargo build --release can build our app BEFORE it builds all the deps, so I think we are pulling in way more than we need.
- RUST: clean up build warnings like unused constructs
- snooze button in notification
- Allow individual streamers to be configured, first with priority
- add a cog button to notification, which opens up the configuration pain for that streamer, outside of the context of the rest of settings, and allows you to create or update the individualised streamer settings
- running make run. Looks like it doesn't support emoji. I think it completely broke notifications / updates? App is still running but I'm not seeing updates. It took the livestream update (this is Hasan, and I see him live), but no updates after that.
--
thread 'tokio-runtime-worker' (242242) panicked at src/notify.rs:185:28:
byte index 47 is not a char boundary; it is inside '👺' (bytes 44..48) of `👺IM BACK👺IN QATAR👺PARTIAL SHUTDOWN?👺EPSTEIN REVEALS POG👺YANIS VAROUFAKIS👺 !guest`
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
