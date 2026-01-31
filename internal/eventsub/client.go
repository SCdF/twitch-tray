package eventsub

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

const (
	eventSubURL         = "wss://eventsub.wss.twitch.tv/ws"
	reconnectBaseDelay  = 1 * time.Second
	reconnectMaxDelay   = 30 * time.Second
	keepaliveTimeoutMul = 1.5 // Multiply keepalive timeout for grace period
)

// MessageType represents the type of EventSub message
type MessageType string

const (
	MessageTypeWelcome      MessageType = "session_welcome"
	MessageTypeKeepalive    MessageType = "session_keepalive"
	MessageTypeNotification MessageType = "notification"
	MessageTypeReconnect    MessageType = "session_reconnect"
	MessageTypeRevocation   MessageType = "revocation"
)

// Message represents a generic EventSub message
type Message struct {
	Metadata Metadata        `json:"metadata"`
	Payload  json.RawMessage `json:"payload"`
}

// Metadata contains message metadata
type Metadata struct {
	MessageID           string      `json:"message_id"`
	MessageType         MessageType `json:"message_type"`
	MessageTimestamp    string      `json:"message_timestamp"`
	SubscriptionType    string      `json:"subscription_type,omitempty"`
	SubscriptionVersion string      `json:"subscription_version,omitempty"`
}

// WelcomePayload is the payload for session_welcome messages
type WelcomePayload struct {
	Session Session `json:"session"`
}

// Session contains session information
type Session struct {
	ID                      string `json:"id"`
	Status                  string `json:"status"`
	ConnectedAt             string `json:"connected_at"`
	KeepaliveTimeoutSeconds int    `json:"keepalive_timeout_seconds"`
	ReconnectURL            string `json:"reconnect_url,omitempty"`
}

// ReconnectPayload is the payload for session_reconnect messages
type ReconnectPayload struct {
	Session Session `json:"session"`
}

// NotificationPayload is the payload for notification messages
type NotificationPayload struct {
	Subscription Subscription    `json:"subscription"`
	Event        json.RawMessage `json:"event"`
}

// Subscription contains subscription information
type Subscription struct {
	ID        string            `json:"id"`
	Status    string            `json:"status"`
	Type      string            `json:"type"`
	Version   string            `json:"version"`
	Condition map[string]string `json:"condition"`
	Transport Transport         `json:"transport"`
	CreatedAt string            `json:"created_at"`
	Cost      int               `json:"cost"`
}

// Transport contains transport information
type Transport struct {
	Method    string `json:"method"`
	SessionID string `json:"session_id"`
}

// EventHandler is called when an event is received
type EventHandler func(eventType string, event json.RawMessage)

// Client manages the EventSub WebSocket connection
type Client struct {
	mu sync.RWMutex

	clientID    string
	accessToken string

	conn      *websocket.Conn
	sessionID string

	keepaliveTimeout time.Duration
	lastMessage      time.Time
	reconnectURL     string

	handlers    []EventHandler
	onConnected func(sessionID string)

	ctx    context.Context
	cancel context.CancelFunc
	wg     sync.WaitGroup
}

// NewClient creates a new EventSub client
func NewClient(clientID, accessToken string) *Client {
	return &Client{
		clientID:    clientID,
		accessToken: accessToken,
	}
}

// OnEvent registers an event handler
func (c *Client) OnEvent(handler EventHandler) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.handlers = append(c.handlers, handler)
}

// OnConnected registers a callback for when the connection is established
func (c *Client) OnConnected(handler func(sessionID string)) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.onConnected = handler
}

// GetSessionID returns the current session ID
func (c *Client) GetSessionID() string {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.sessionID
}

// Connect establishes the WebSocket connection
func (c *Client) Connect(ctx context.Context) error {
	c.mu.Lock()
	c.ctx, c.cancel = context.WithCancel(ctx)
	c.mu.Unlock()

	return c.connectWithRetry()
}

func (c *Client) connectWithRetry() error {
	delay := reconnectBaseDelay

	for {
		select {
		case <-c.ctx.Done():
			return c.ctx.Err()
		default:
		}

		url := eventSubURL
		c.mu.RLock()
		if c.reconnectURL != "" {
			url = c.reconnectURL
		}
		c.mu.RUnlock()

		err := c.connect(url)
		if err == nil {
			return nil
		}

		log.Printf("EventSub connection failed: %v, retrying in %v", err, delay)

		select {
		case <-c.ctx.Done():
			return c.ctx.Err()
		case <-time.After(delay):
		}

		// Exponential backoff
		delay *= 2
		if delay > reconnectMaxDelay {
			delay = reconnectMaxDelay
		}
	}
}

func (c *Client) connect(url string) error {
	conn, _, err := websocket.DefaultDialer.Dial(url, nil)
	if err != nil {
		return fmt.Errorf("failed to connect: %w", err)
	}

	c.mu.Lock()
	c.conn = conn
	c.lastMessage = time.Now()
	c.mu.Unlock()

	// Start message reader
	c.wg.Add(1)
	go c.readMessages()

	// Start keepalive monitor
	c.wg.Add(1)
	go c.monitorKeepalive()

	return nil
}

func (c *Client) readMessages() {
	defer c.wg.Done()

	for {
		select {
		case <-c.ctx.Done():
			return
		default:
		}

		c.mu.RLock()
		conn := c.conn
		c.mu.RUnlock()

		if conn == nil {
			return
		}

		_, data, err := conn.ReadMessage()
		if err != nil {
			if websocket.IsCloseError(err, websocket.CloseNormalClosure) {
				return
			}
			log.Printf("EventSub read error: %v", err)
			c.handleDisconnect()
			return
		}

		c.mu.Lock()
		c.lastMessage = time.Now()
		c.mu.Unlock()

		c.handleMessage(data)
	}
}

func (c *Client) handleMessage(data []byte) {
	var msg Message
	if err := json.Unmarshal(data, &msg); err != nil {
		log.Printf("EventSub parse error: %v", err)
		return
	}

	switch msg.Metadata.MessageType {
	case MessageTypeWelcome:
		c.handleWelcome(msg.Payload)
	case MessageTypeKeepalive:
		// Just updates lastMessage, already done above
	case MessageTypeNotification:
		c.handleNotification(msg.Payload)
	case MessageTypeReconnect:
		c.handleReconnect(msg.Payload)
	case MessageTypeRevocation:
		c.handleRevocation(msg.Payload)
	}
}

func (c *Client) handleWelcome(payload json.RawMessage) {
	var welcome WelcomePayload
	if err := json.Unmarshal(payload, &welcome); err != nil {
		log.Printf("EventSub welcome parse error: %v", err)
		return
	}

	c.mu.Lock()
	c.sessionID = welcome.Session.ID
	c.keepaliveTimeout = time.Duration(float64(welcome.Session.KeepaliveTimeoutSeconds)*keepaliveTimeoutMul) * time.Second
	onConnected := c.onConnected
	c.mu.Unlock()

	log.Printf("EventSub connected, session: %s, keepalive: %ds",
		welcome.Session.ID, welcome.Session.KeepaliveTimeoutSeconds)

	if onConnected != nil {
		onConnected(welcome.Session.ID)
	}
}

func (c *Client) handleNotification(payload json.RawMessage) {
	var notif NotificationPayload
	if err := json.Unmarshal(payload, &notif); err != nil {
		log.Printf("EventSub notification parse error: %v", err)
		return
	}

	c.mu.RLock()
	handlers := make([]EventHandler, len(c.handlers))
	copy(handlers, c.handlers)
	c.mu.RUnlock()

	for _, handler := range handlers {
		handler(notif.Subscription.Type, notif.Event)
	}
}

func (c *Client) handleReconnect(payload json.RawMessage) {
	var reconnect ReconnectPayload
	if err := json.Unmarshal(payload, &reconnect); err != nil {
		log.Printf("EventSub reconnect parse error: %v", err)
		return
	}

	c.mu.Lock()
	c.reconnectURL = reconnect.Session.ReconnectURL
	c.mu.Unlock()

	log.Printf("EventSub reconnect requested to: %s", reconnect.Session.ReconnectURL)

	// Close current connection and reconnect
	c.handleDisconnect()
}

func (c *Client) handleRevocation(payload json.RawMessage) {
	var notif NotificationPayload
	if err := json.Unmarshal(payload, &notif); err != nil {
		log.Printf("EventSub revocation parse error: %v", err)
		return
	}

	log.Printf("EventSub subscription revoked: %s (%s)", notif.Subscription.Type, notif.Subscription.Status)
}

func (c *Client) handleDisconnect() {
	c.mu.Lock()
	if c.conn != nil {
		c.conn.Close()
		c.conn = nil
	}
	c.mu.Unlock()

	// Attempt to reconnect
	go c.connectWithRetry()
}

func (c *Client) monitorKeepalive() {
	defer c.wg.Done()

	ticker := time.NewTicker(5 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-c.ctx.Done():
			return
		case <-ticker.C:
			c.mu.RLock()
			timeout := c.keepaliveTimeout
			lastMsg := c.lastMessage
			c.mu.RUnlock()

			if timeout > 0 && time.Since(lastMsg) > timeout {
				log.Printf("EventSub keepalive timeout")
				c.handleDisconnect()
				return
			}
		}
	}
}

// Close closes the WebSocket connection
func (c *Client) Close() error {
	c.mu.Lock()
	if c.cancel != nil {
		c.cancel()
	}
	conn := c.conn
	c.conn = nil
	c.mu.Unlock()

	if conn != nil {
		conn.WriteMessage(websocket.CloseMessage,
			websocket.FormatCloseMessage(websocket.CloseNormalClosure, ""))
		conn.Close()
	}

	c.wg.Wait()
	return nil
}
