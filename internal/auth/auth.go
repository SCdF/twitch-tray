package auth

import (
	"encoding/json"
	"errors"
	"time"

	"github.com/99designs/keyring"
)

const (
	serviceName = "twitch-tray"
	tokenKey    = "oauth_token"

	// ClientID is the Twitch application client ID
	ClientID = "w1kicz6atgkpl5jbwtq5tj2u4vd2i7"
)

var (
	ErrNoToken      = errors.New("no token stored")
	ErrTokenExpired = errors.New("token expired")
)

// Token represents OAuth tokens for Twitch
type Token struct {
	AccessToken  string    `json:"access_token"`
	RefreshToken string    `json:"refresh_token"`
	ExpiresAt    time.Time `json:"expires_at"`
	Scopes       []string  `json:"scopes"`
	UserID       string    `json:"user_id"`
	UserLogin    string    `json:"user_login"`
}

// IsExpired checks if the token has expired
func (t *Token) IsExpired() bool {
	return time.Now().After(t.ExpiresAt)
}

// IsValid checks if the token exists and is not expired
func (t *Token) IsValid() bool {
	return t.AccessToken != "" && !t.IsExpired()
}

// Store handles secure token storage using the system keyring
type Store struct {
	ring keyring.Keyring
}

// NewStore creates a new token store
func NewStore() (*Store, error) {
	ring, err := keyring.Open(keyring.Config{
		ServiceName: serviceName,
		// Use appropriate backend based on platform
		AllowedBackends: []keyring.BackendType{
			keyring.SecretServiceBackend,  // Linux
			keyring.KeychainBackend,       // macOS
			keyring.WinCredBackend,        // Windows
			keyring.PassBackend,           // Linux fallback
			keyring.FileBackend,           // Universal fallback
		},
		FileDir:                  "~/.twitch-tray-keys",
		FilePasswordFunc:         keyring.FixedStringPrompt("twitch-tray"),
		LibSecretCollectionName:  serviceName,
		KWalletAppID:             serviceName,
		KWalletFolder:            serviceName,
		KeychainTrustApplication: true,
	})
	if err != nil {
		return nil, err
	}

	return &Store{ring: ring}, nil
}

// SaveToken stores the OAuth token securely
func (s *Store) SaveToken(token *Token) error {
	data, err := json.Marshal(token)
	if err != nil {
		return err
	}

	return s.ring.Set(keyring.Item{
		Key:  tokenKey,
		Data: data,
	})
}

// LoadToken retrieves the stored OAuth token
func (s *Store) LoadToken() (*Token, error) {
	item, err := s.ring.Get(tokenKey)
	if err != nil {
		if errors.Is(err, keyring.ErrKeyNotFound) {
			return nil, ErrNoToken
		}
		return nil, err
	}

	var token Token
	if err := json.Unmarshal(item.Data, &token); err != nil {
		return nil, err
	}

	return &token, nil
}

// DeleteToken removes the stored token
func (s *Store) DeleteToken() error {
	err := s.ring.Remove(tokenKey)
	if errors.Is(err, keyring.ErrKeyNotFound) {
		return nil // Already deleted
	}
	return err
}

// HasToken checks if a token is stored
func (s *Store) HasToken() bool {
	_, err := s.ring.Get(tokenKey)
	return err == nil
}
