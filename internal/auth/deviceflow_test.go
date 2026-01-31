package auth

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"
)

func TestWaitForToken_PollsMultipleTimes(t *testing.T) {
	var pollCount int32

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		count := atomic.AddInt32(&pollCount, 1)

		// Return pending for first 3 polls, then success
		if count < 4 {
			w.WriteHeader(http.StatusBadRequest)
			json.NewEncoder(w).Encode(map[string]interface{}{
				"status":  400,
				"message": "authorization_pending",
			})
			return
		}

		// Success response
		w.WriteHeader(http.StatusOK)
		json.NewEncoder(w).Encode(map[string]interface{}{
			"access_token":  "test_token",
			"refresh_token": "test_refresh",
			"expires_in":    14400,
			"scope":         []string{"user:read:follows"},
			"token_type":    "bearer",
		})
	}))
	defer server.Close()

	// Override the token URL for testing
	originalURL := tokenURL
	tokenURL = server.URL
	defer func() { tokenURL = originalURL }()

	flow := NewDeviceFlow("test_client_id")
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	dcr := &DeviceCodeResponse{
		DeviceCode: "test_device_code",
		ExpiresIn:  1800,
		Interval:   1, // 1 second interval for faster testing
	}

	token, err := flow.WaitForToken(ctx, dcr)
	if err != nil {
		t.Fatalf("WaitForToken failed: %v", err)
	}

	if token == nil {
		t.Fatal("Expected token, got nil")
	}

	if token.AccessToken != "test_token" {
		t.Errorf("Expected access_token 'test_token', got '%s'", token.AccessToken)
	}

	if pollCount < 4 {
		t.Errorf("Expected at least 4 polls, got %d", pollCount)
	}

	t.Logf("Polled %d times before success", pollCount)
}

func TestWaitForToken_HandlesSlowDown(t *testing.T) {
	var pollCount int32

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		count := atomic.AddInt32(&pollCount, 1)

		// Return slow_down on first poll, then pending, then success
		if count == 1 {
			w.WriteHeader(http.StatusBadRequest)
			json.NewEncoder(w).Encode(map[string]interface{}{
				"status":  400,
				"message": "slow_down",
			})
			return
		}

		if count < 3 {
			w.WriteHeader(http.StatusBadRequest)
			json.NewEncoder(w).Encode(map[string]interface{}{
				"status":  400,
				"message": "authorization_pending",
			})
			return
		}

		w.WriteHeader(http.StatusOK)
		json.NewEncoder(w).Encode(map[string]interface{}{
			"access_token":  "test_token",
			"refresh_token": "test_refresh",
			"expires_in":    14400,
			"scope":         []string{"user:read:follows"},
			"token_type":    "bearer",
		})
	}))
	defer server.Close()

	originalURL := tokenURL
	tokenURL = server.URL
	defer func() { tokenURL = originalURL }()

	flow := NewDeviceFlow("test_client_id")
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	dcr := &DeviceCodeResponse{
		DeviceCode: "test_device_code",
		ExpiresIn:  1800,
		Interval:   1,
	}

	token, err := flow.WaitForToken(ctx, dcr)
	if err != nil {
		t.Fatalf("WaitForToken failed: %v", err)
	}

	if token == nil {
		t.Fatal("Expected token, got nil")
	}

	if pollCount < 3 {
		t.Errorf("Expected at least 3 polls, got %d", pollCount)
	}

	t.Logf("Polled %d times with slow_down handling", pollCount)
}

func TestWaitForToken_ContextCancellation(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		json.NewEncoder(w).Encode(map[string]interface{}{
			"status":  400,
			"message": "authorization_pending",
		})
	}))
	defer server.Close()

	originalURL := tokenURL
	tokenURL = server.URL
	defer func() { tokenURL = originalURL }()

	flow := NewDeviceFlow("test_client_id")
	ctx, cancel := context.WithCancel(context.Background())

	dcr := &DeviceCodeResponse{
		DeviceCode: "test_device_code",
		ExpiresIn:  1800,
		Interval:   1,
	}

	// Cancel after a short delay
	go func() {
		time.Sleep(500 * time.Millisecond)
		cancel()
	}()

	_, err := flow.WaitForToken(ctx, dcr)
	if err != context.Canceled {
		t.Errorf("Expected context.Canceled error, got: %v", err)
	}
}

func TestWaitForToken_AccessDenied(t *testing.T) {
	var pollCount int32

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		count := atomic.AddInt32(&pollCount, 1)

		// Pending first, then denied
		if count < 2 {
			w.WriteHeader(http.StatusBadRequest)
			json.NewEncoder(w).Encode(map[string]interface{}{
				"status":  400,
				"message": "authorization_pending",
			})
			return
		}

		w.WriteHeader(http.StatusBadRequest)
		json.NewEncoder(w).Encode(map[string]interface{}{
			"status":  400,
			"message": "access_denied",
		})
	}))
	defer server.Close()

	originalURL := tokenURL
	tokenURL = server.URL
	defer func() { tokenURL = originalURL }()

	flow := NewDeviceFlow("test_client_id")
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	dcr := &DeviceCodeResponse{
		DeviceCode: "test_device_code",
		ExpiresIn:  1800,
		Interval:   1,
	}

	_, err := flow.WaitForToken(ctx, dcr)
	if err != ErrAccessDenied {
		t.Errorf("Expected ErrAccessDenied, got: %v", err)
	}

	if pollCount < 2 {
		t.Errorf("Expected at least 2 polls before denial, got %d", pollCount)
	}
}
