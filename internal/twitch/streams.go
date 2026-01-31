package twitch

import (
	"context"
	"fmt"

	"github.com/nicklaw5/helix/v2"
)

// GetFollowedStreams retrieves live streams from channels the user follows
func (c *Client) GetFollowedStreams(ctx context.Context) ([]Stream, error) {
	c.mu.RLock()
	client := c.helix
	userID := c.userID
	c.mu.RUnlock()

	if userID == "" {
		return nil, fmt.Errorf("user ID not set")
	}

	var allStreams []Stream
	cursor := ""

	for {
		resp, err := client.GetFollowedStream(&helix.FollowedStreamsParams{
			UserID: userID,
			First:  100,
			After:  cursor,
		})
		if err != nil {
			return nil, err
		}

		if resp.ErrorStatus != 0 {
			return nil, fmt.Errorf("API error %d: %s", resp.ErrorStatus, resp.ErrorMessage)
		}

		for _, s := range resp.Data.Streams {
			allStreams = append(allStreams, Stream{
				ID:           s.ID,
				UserID:       s.UserID,
				UserLogin:    s.UserLogin,
				UserName:     s.UserName,
				GameID:       s.GameID,
				GameName:     s.GameName,
				Title:        s.Title,
				ViewerCount:  s.ViewerCount,
				StartedAt:    s.StartedAt,
				ThumbnailURL: s.ThumbnailURL,
				Tags:         s.Tags,
			})
		}

		if resp.Data.Pagination.Cursor == "" {
			break
		}
		cursor = resp.Data.Pagination.Cursor

		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		default:
		}
	}

	return allStreams, nil
}

// GetStreamsByGameID retrieves streams for a specific game/category
func (c *Client) GetStreamsByGameID(ctx context.Context, gameID string, limit int) ([]Stream, error) {
	c.mu.RLock()
	client := c.helix
	c.mu.RUnlock()

	if limit <= 0 {
		limit = 5
	}
	if limit > 100 {
		limit = 100
	}

	resp, err := client.GetStreams(&helix.StreamsParams{
		GameIDs: []string{gameID},
		First:   limit,
	})
	if err != nil {
		return nil, err
	}

	if resp.ErrorStatus != 0 {
		return nil, fmt.Errorf("API error %d: %s", resp.ErrorStatus, resp.ErrorMessage)
	}

	streams := make([]Stream, 0, len(resp.Data.Streams))
	for _, s := range resp.Data.Streams {
		streams = append(streams, Stream{
			ID:           s.ID,
			UserID:       s.UserID,
			UserLogin:    s.UserLogin,
			UserName:     s.UserName,
			GameID:       s.GameID,
			GameName:     s.GameName,
			Title:        s.Title,
			ViewerCount:  s.ViewerCount,
			StartedAt:    s.StartedAt,
			ThumbnailURL: s.ThumbnailURL,
			Tags:         s.Tags,
		})
	}

	return streams, nil
}

// GetStreamsByUserIDs retrieves streams for specific users
func (c *Client) GetStreamsByUserIDs(ctx context.Context, userIDs []string) ([]Stream, error) {
	c.mu.RLock()
	client := c.helix
	c.mu.RUnlock()

	if len(userIDs) == 0 {
		return nil, nil
	}

	// API allows max 100 user IDs per request
	var allStreams []Stream
	for i := 0; i < len(userIDs); i += 100 {
		end := i + 100
		if end > len(userIDs) {
			end = len(userIDs)
		}
		batch := userIDs[i:end]

		resp, err := client.GetStreams(&helix.StreamsParams{
			UserIDs: batch,
			First:   100,
		})
		if err != nil {
			return nil, err
		}

		if resp.ErrorStatus != 0 {
			return nil, fmt.Errorf("API error %d: %s", resp.ErrorStatus, resp.ErrorMessage)
		}

		for _, s := range resp.Data.Streams {
			allStreams = append(allStreams, Stream{
				ID:           s.ID,
				UserID:       s.UserID,
				UserLogin:    s.UserLogin,
				UserName:     s.UserName,
				GameID:       s.GameID,
				GameName:     s.GameName,
				Title:        s.Title,
				ViewerCount:  s.ViewerCount,
				StartedAt:    s.StartedAt,
				ThumbnailURL: s.ThumbnailURL,
				Tags:         s.Tags,
			})
		}

		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		default:
		}
	}

	return allStreams, nil
}

// GetGames retrieves game/category information by IDs
func (c *Client) GetGames(ctx context.Context, gameIDs []string) ([]Category, error) {
	c.mu.RLock()
	client := c.helix
	c.mu.RUnlock()

	if len(gameIDs) == 0 {
		return nil, nil
	}

	resp, err := client.GetGames(&helix.GamesParams{
		IDs: gameIDs,
	})
	if err != nil {
		return nil, err
	}

	if resp.ErrorStatus != 0 {
		return nil, fmt.Errorf("API error %d: %s", resp.ErrorStatus, resp.ErrorMessage)
	}

	categories := make([]Category, 0, len(resp.Data.Games))
	for _, g := range resp.Data.Games {
		categories = append(categories, Category{
			ID:        g.ID,
			Name:      g.Name,
			BoxArtURL: g.BoxArtURL,
		})
	}

	return categories, nil
}
