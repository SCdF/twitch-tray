package twitch

import (
	"context"
	"fmt"
	"sync"

	"github.com/nicklaw5/helix/v2"
)

// Client wraps the Helix API client with additional functionality
type Client struct {
	mu       sync.RWMutex
	helix    *helix.Client
	clientID string
	userID   string
}

// NewClient creates a new Twitch API client
func NewClient(clientID string) (*Client, error) {
	client, err := helix.NewClient(&helix.Options{
		ClientID: clientID,
	})
	if err != nil {
		return nil, fmt.Errorf("failed to create helix client: %w", err)
	}

	return &Client{
		helix:    client,
		clientID: clientID,
	}, nil
}

// SetAccessToken sets the access token for API requests
func (c *Client) SetAccessToken(token string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.helix.SetUserAccessToken(token)
}

// SetUserID sets the authenticated user's ID
func (c *Client) SetUserID(userID string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.userID = userID
}

// GetUserID returns the authenticated user's ID
func (c *Client) GetUserID() string {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.userID
}

// GetClientID returns the client ID
func (c *Client) GetClientID() string {
	return c.clientID
}

// GetHelix returns the underlying helix client for direct access
func (c *Client) GetHelix() *helix.Client {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.helix
}

// GetUsers retrieves user information by IDs or logins
func (c *Client) GetUsers(ctx context.Context, ids []string, logins []string) ([]helix.User, error) {
	c.mu.RLock()
	client := c.helix
	c.mu.RUnlock()

	resp, err := client.GetUsers(&helix.UsersParams{
		IDs:    ids,
		Logins: logins,
	})
	if err != nil {
		return nil, err
	}

	if resp.ErrorStatus != 0 {
		return nil, fmt.Errorf("API error %d: %s", resp.ErrorStatus, resp.ErrorMessage)
	}

	return resp.Data.Users, nil
}

// GetFollowedChannels retrieves channels the user follows
func (c *Client) GetFollowedChannels(ctx context.Context, cursor string) ([]helix.ChannelFollow, string, error) {
	c.mu.RLock()
	client := c.helix
	userID := c.userID
	c.mu.RUnlock()

	if userID == "" {
		return nil, "", fmt.Errorf("user ID not set")
	}

	resp, err := client.GetChannelFollows(&helix.GetChannelFollowsParams{
		UserID: userID,
		First:  100,
		After:  cursor,
	})
	if err != nil {
		return nil, "", err
	}

	if resp.ErrorStatus != 0 {
		return nil, "", fmt.Errorf("API error %d: %s", resp.ErrorStatus, resp.ErrorMessage)
	}

	return resp.Data.Channels, resp.Data.Pagination.Cursor, nil
}

// GetAllFollowedChannels retrieves all channels the user follows (handles pagination)
func (c *Client) GetAllFollowedChannels(ctx context.Context) ([]helix.ChannelFollow, error) {
	var allFollows []helix.ChannelFollow
	cursor := ""

	for {
		follows, nextCursor, err := c.GetFollowedChannels(ctx, cursor)
		if err != nil {
			return nil, err
		}

		allFollows = append(allFollows, follows...)

		if nextCursor == "" {
			break
		}
		cursor = nextCursor

		// Check context for cancellation
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		default:
		}
	}

	return allFollows, nil
}
