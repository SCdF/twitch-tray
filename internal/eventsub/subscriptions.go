package eventsub

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

const (
	subscribeURL = "https://api.twitch.tv/helix/eventsub/subscriptions"
)

// SubscriptionType represents EventSub subscription types
type SubscriptionType string

const (
	SubStreamOnline   SubscriptionType = "stream.online"
	SubStreamOffline  SubscriptionType = "stream.offline"
	SubChannelUpdate  SubscriptionType = "channel.update"
)

// CreateSubscriptionRequest is the request body for creating a subscription
type CreateSubscriptionRequest struct {
	Type      string            `json:"type"`
	Version   string            `json:"version"`
	Condition map[string]string `json:"condition"`
	Transport TransportRequest  `json:"transport"`
}

// TransportRequest is the transport configuration for a subscription
type TransportRequest struct {
	Method    string `json:"method"`
	SessionID string `json:"session_id"`
}

// CreateSubscriptionResponse is the response from creating a subscription
type CreateSubscriptionResponse struct {
	Data         []Subscription `json:"data"`
	Total        int            `json:"total"`
	TotalCost    int            `json:"total_cost"`
	MaxTotalCost int            `json:"max_total_cost"`
}

// SubscriptionManager manages EventSub subscriptions
type SubscriptionManager struct {
	clientID    string
	accessToken string
	sessionID   string
	httpClient  *http.Client

	subscriptions map[string]string // type:broadcasterID -> subscriptionID
}

// NewSubscriptionManager creates a new subscription manager
func NewSubscriptionManager(clientID, accessToken string) *SubscriptionManager {
	return &SubscriptionManager{
		clientID:      clientID,
		accessToken:   accessToken,
		httpClient:    &http.Client{Timeout: 10 * time.Second},
		subscriptions: make(map[string]string),
	}
}

// SetSessionID sets the WebSocket session ID for subscriptions
func (m *SubscriptionManager) SetSessionID(sessionID string) {
	m.sessionID = sessionID
}

// SubscribeToChannel creates subscriptions for a broadcaster
func (m *SubscriptionManager) SubscribeToChannel(ctx context.Context, broadcasterID string) error {
	if m.sessionID == "" {
		return fmt.Errorf("session ID not set")
	}

	// Subscribe to stream.online
	if err := m.createSubscription(ctx, SubStreamOnline, broadcasterID); err != nil {
		return fmt.Errorf("failed to subscribe to stream.online: %w", err)
	}

	// Subscribe to stream.offline
	if err := m.createSubscription(ctx, SubStreamOffline, broadcasterID); err != nil {
		return fmt.Errorf("failed to subscribe to stream.offline: %w", err)
	}

	// Subscribe to channel.update (for category changes)
	if err := m.createSubscription(ctx, SubChannelUpdate, broadcasterID); err != nil {
		return fmt.Errorf("failed to subscribe to channel.update: %w", err)
	}

	return nil
}

// SubscribeToChannels creates subscriptions for multiple broadcasters
func (m *SubscriptionManager) SubscribeToChannels(ctx context.Context, broadcasterIDs []string) error {
	for _, id := range broadcasterIDs {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		if err := m.SubscribeToChannel(ctx, id); err != nil {
			// Log but continue with other channels
			fmt.Printf("Warning: failed to subscribe to channel %s: %v\n", id, err)
		}
	}
	return nil
}

func (m *SubscriptionManager) createSubscription(ctx context.Context, subType SubscriptionType, broadcasterID string) error {
	key := fmt.Sprintf("%s:%s", subType, broadcasterID)

	// Check if already subscribed
	if _, exists := m.subscriptions[key]; exists {
		return nil
	}

	req := CreateSubscriptionRequest{
		Type:    string(subType),
		Version: "1",
		Condition: map[string]string{
			"broadcaster_user_id": broadcasterID,
		},
		Transport: TransportRequest{
			Method:    "websocket",
			SessionID: m.sessionID,
		},
	}

	body, err := json.Marshal(req)
	if err != nil {
		return err
	}

	httpReq, err := http.NewRequestWithContext(ctx, "POST", subscribeURL, bytes.NewReader(body))
	if err != nil {
		return err
	}

	httpReq.Header.Set("Authorization", "Bearer "+m.accessToken)
	httpReq.Header.Set("Client-Id", m.clientID)
	httpReq.Header.Set("Content-Type", "application/json")

	resp, err := m.httpClient.Do(httpReq)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	// 409 Conflict means subscription already exists, which is fine
	if resp.StatusCode == http.StatusConflict {
		return nil
	}

	if resp.StatusCode != http.StatusAccepted {
		respBody, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("subscription failed (%d): %s", resp.StatusCode, string(respBody))
	}

	var subResp CreateSubscriptionResponse
	if err := json.NewDecoder(resp.Body).Decode(&subResp); err != nil {
		return err
	}

	if len(subResp.Data) > 0 {
		m.subscriptions[key] = subResp.Data[0].ID
	}

	return nil
}

// DeleteSubscription removes a subscription
func (m *SubscriptionManager) DeleteSubscription(ctx context.Context, subscriptionID string) error {
	req, err := http.NewRequestWithContext(ctx, "DELETE", subscribeURL+"?id="+subscriptionID, nil)
	if err != nil {
		return err
	}

	req.Header.Set("Authorization", "Bearer "+m.accessToken)
	req.Header.Set("Client-Id", m.clientID)

	resp, err := m.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusNoContent && resp.StatusCode != http.StatusNotFound {
		return fmt.Errorf("delete subscription failed: %s", resp.Status)
	}

	return nil
}

// ClearSubscriptions removes all tracked subscriptions
func (m *SubscriptionManager) ClearSubscriptions(ctx context.Context) {
	for key, id := range m.subscriptions {
		_ = m.DeleteSubscription(ctx, id)
		delete(m.subscriptions, key)
	}
}
