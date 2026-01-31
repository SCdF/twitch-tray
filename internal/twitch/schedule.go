package twitch

import (
	"context"
	"fmt"
	"sort"
	"time"

	"github.com/nicklaw5/helix/v2"
)

// GetScheduledStreams retrieves scheduled broadcasts for followed channels in the next 24 hours
func (c *Client) GetScheduledStreams(ctx context.Context, broadcasterIDs []string) ([]ScheduledStream, error) {
	if len(broadcasterIDs) == 0 {
		return nil, nil
	}

	c.mu.RLock()
	client := c.helix
	c.mu.RUnlock()

	now := time.Now()
	cutoff := now.Add(24 * time.Hour)
	var allScheduled []ScheduledStream

	// Get schedules for each broadcaster
	// Note: The API only allows one broadcaster ID at a time for schedules
	for _, broadcasterID := range broadcasterIDs {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		default:
		}

		resp, err := client.GetSchedule(&helix.GetScheduleParams{
			BroadcasterID: broadcasterID,
			First:         10, // Get next 10 scheduled segments
		})
		if err != nil {
			// Skip this broadcaster if there's an error (they might not have a schedule)
			continue
		}

		if resp.ErrorStatus != 0 {
			// 404 means no schedule, which is fine
			if resp.ErrorStatus == 404 {
				continue
			}
			continue // Skip other errors too
		}

		schedule := resp.Data.Schedule
		for _, segment := range schedule.Segments {
			startTime := segment.StartTime.Time

			// Skip if already started or past our 24h window
			if startTime.Before(now) || startTime.After(cutoff) {
				continue
			}

			// Skip canceled segments
			if segment.CanceledUntil != "" {
				continue
			}

			scheduled := ScheduledStream{
				ID:               segment.ID,
				BroadcasterID:    schedule.BroadcasterID,
				BroadcasterName:  schedule.BroadcasterName,
				BroadcasterLogin: schedule.BroadcasterLogin,
				Title:            segment.Title,
				StartTime:        startTime,
				IsRecurring:      segment.IsRecurring,
			}

			if segment.EndTime.Time.After(startTime) {
				scheduled.EndTime = segment.EndTime.Time
			}

			if segment.Category.ID != "" {
				scheduled.Category = segment.Category.Name
				scheduled.CategoryID = segment.Category.ID
			}

			allScheduled = append(allScheduled, scheduled)
		}
	}

	// Sort by start time
	sort.Slice(allScheduled, func(i, j int) bool {
		return allScheduled[i].StartTime.Before(allScheduled[j].StartTime)
	})

	return allScheduled, nil
}

// GetScheduledStreamsForFollowed retrieves scheduled streams for all followed channels
func (c *Client) GetScheduledStreamsForFollowed(ctx context.Context) ([]ScheduledStream, error) {
	// First get all followed channels
	follows, err := c.GetAllFollowedChannels(ctx)
	if err != nil {
		return nil, fmt.Errorf("failed to get followed channels: %w", err)
	}

	// Extract broadcaster IDs
	broadcasterIDs := make([]string, 0, len(follows))
	for _, f := range follows {
		broadcasterIDs = append(broadcasterIDs, f.BroadcasterID)
	}

	// Get schedules (limit to avoid too many API calls)
	// In practice, we might want to prioritize or limit this
	maxBroadcasters := 50
	if len(broadcasterIDs) > maxBroadcasters {
		broadcasterIDs = broadcasterIDs[:maxBroadcasters]
	}

	return c.GetScheduledStreams(ctx, broadcasterIDs)
}
