package auth

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const (
	deviceCodeURL = "https://id.twitch.tv/oauth2/device"
	tokenURL      = "https://id.twitch.tv/oauth2/token"
	validateURL   = "https://id.twitch.tv/oauth2/validate"

	// Required scopes for the application
	requiredScopes = "user:read:follows"
)

var (
	ErrAuthorizationPending = errors.New("authorization pending")
	ErrSlowDown             = errors.New("slow down")
	ErrAccessDenied         = errors.New("access denied by user")
	ErrExpiredToken         = errors.New("device code expired")
)

// DeviceCodeResponse is the response from the device code request
type DeviceCodeResponse struct {
	DeviceCode      string `json:"device_code"`
	UserCode        string `json:"user_code"`
	VerificationURI string `json:"verification_uri"`
	ExpiresIn       int    `json:"expires_in"`
	Interval        int    `json:"interval"`
}

// TokenResponse is the response from the token request
type TokenResponse struct {
	AccessToken  string `json:"access_token"`
	RefreshToken string `json:"refresh_token"`
	ExpiresIn    int    `json:"expires_in"`
	Scope        string `json:"scope"`
	TokenType    string `json:"token_type"`
}

// ValidateResponse is the response from token validation
type ValidateResponse struct {
	ClientID  string   `json:"client_id"`
	Login     string   `json:"login"`
	Scopes    []string `json:"scopes"`
	UserID    string   `json:"user_id"`
	ExpiresIn int      `json:"expires_in"`
}

// DeviceFlow handles the OAuth Device Code Flow
type DeviceFlow struct {
	clientID   string
	httpClient *http.Client
}

// NewDeviceFlow creates a new device flow handler
func NewDeviceFlow(clientID string) *DeviceFlow {
	return &DeviceFlow{
		clientID: clientID,
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

// RequestDeviceCode initiates the device code flow
func (d *DeviceFlow) RequestDeviceCode(ctx context.Context) (*DeviceCodeResponse, error) {
	data := url.Values{}
	data.Set("client_id", d.clientID)
	data.Set("scopes", requiredScopes)

	req, err := http.NewRequestWithContext(ctx, "POST", deviceCodeURL, strings.NewReader(data.Encode()))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	resp, err := d.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("device code request failed: %s", resp.Status)
	}

	var dcr DeviceCodeResponse
	if err := json.NewDecoder(resp.Body).Decode(&dcr); err != nil {
		return nil, err
	}

	return &dcr, nil
}

// PollForToken polls for the access token
func (d *DeviceFlow) PollForToken(ctx context.Context, deviceCode string) (*TokenResponse, error) {
	data := url.Values{}
	data.Set("client_id", d.clientID)
	data.Set("device_code", deviceCode)
	data.Set("grant_type", "urn:ietf:params:oauth:grant-type:device_code")

	req, err := http.NewRequestWithContext(ctx, "POST", tokenURL, strings.NewReader(data.Encode()))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	resp, err := d.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	// Check for pending/error states
	if resp.StatusCode == http.StatusBadRequest {
		var errResp struct {
			Status  int    `json:"status"`
			Message string `json:"message"`
		}
		if err := json.NewDecoder(resp.Body).Decode(&errResp); err != nil {
			return nil, err
		}

		switch errResp.Message {
		case "authorization_pending":
			return nil, ErrAuthorizationPending
		case "slow_down":
			return nil, ErrSlowDown
		case "access_denied":
			return nil, ErrAccessDenied
		case "expired_token":
			return nil, ErrExpiredToken
		default:
			return nil, fmt.Errorf("token error: %s", errResp.Message)
		}
	}

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("token request failed: %s", resp.Status)
	}

	var tr TokenResponse
	if err := json.NewDecoder(resp.Body).Decode(&tr); err != nil {
		return nil, err
	}

	return &tr, nil
}

// WaitForToken polls until the user authorizes or the code expires
func (d *DeviceFlow) WaitForToken(ctx context.Context, dcr *DeviceCodeResponse) (*TokenResponse, error) {
	interval := time.Duration(dcr.Interval) * time.Second
	if interval == 0 {
		interval = 5 * time.Second
	}

	deadline := time.Now().Add(time.Duration(dcr.ExpiresIn) * time.Second)
	ticker := time.NewTicker(interval)
	defer ticker.Stop()

	log.Printf("Polling for token every %v (expires in %ds)", interval, dcr.ExpiresIn)

	for {
		select {
		case <-ctx.Done():
			log.Printf("Context cancelled: %v", ctx.Err())
			return nil, ctx.Err()
		case <-ticker.C:
			if time.Now().After(deadline) {
				log.Printf("Device code expired")
				return nil, ErrExpiredToken
			}

			tr, err := d.PollForToken(ctx, dcr.DeviceCode)
			if err == nil {
				log.Printf("Token received successfully")
				return tr, nil
			}

			switch {
			case errors.Is(err, ErrAuthorizationPending):
				log.Printf("Authorization pending, continuing to poll...")
				continue // Keep polling
			case errors.Is(err, ErrSlowDown):
				interval += 5 * time.Second
				ticker.Reset(interval)
				log.Printf("Slowing down, new interval: %v", interval)
				continue
			default:
				log.Printf("Poll error: %v", err)
				return nil, err
			}
		}
	}
}

// ValidateToken validates an access token and returns user info
func (d *DeviceFlow) ValidateToken(ctx context.Context, accessToken string) (*ValidateResponse, error) {
	req, err := http.NewRequestWithContext(ctx, "GET", validateURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", "OAuth "+accessToken)

	resp, err := d.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusUnauthorized {
		return nil, ErrTokenExpired
	}

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("validation failed: %s", resp.Status)
	}

	var vr ValidateResponse
	if err := json.NewDecoder(resp.Body).Decode(&vr); err != nil {
		return nil, err
	}

	return &vr, nil
}

// Authenticate performs the full device code flow
func (d *DeviceFlow) Authenticate(ctx context.Context, onCode func(userCode, verificationURI string)) (*Token, error) {
	dcr, err := d.RequestDeviceCode(ctx)
	if err != nil {
		return nil, fmt.Errorf("failed to request device code: %w", err)
	}

	// Notify the caller of the user code
	if onCode != nil {
		onCode(dcr.UserCode, dcr.VerificationURI)
	}

	tr, err := d.WaitForToken(ctx, dcr)
	if err != nil {
		return nil, fmt.Errorf("failed to get token: %w", err)
	}

	// Validate the token to get user info
	vr, err := d.ValidateToken(ctx, tr.AccessToken)
	if err != nil {
		return nil, fmt.Errorf("failed to validate token: %w", err)
	}

	return &Token{
		AccessToken:  tr.AccessToken,
		RefreshToken: tr.RefreshToken,
		ExpiresAt:    time.Now().Add(time.Duration(tr.ExpiresIn) * time.Second),
		Scopes:       strings.Split(tr.Scope, " "),
		UserID:       vr.UserID,
		UserLogin:    vr.Login,
	}, nil
}
