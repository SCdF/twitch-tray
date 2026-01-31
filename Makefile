.PHONY: all build build-linux build-darwin build-windows clean deps run lint test

# Binary name
BINARY=twitch-tray

# Build directory
DIST=dist

# Version (override with: make build VERSION=1.0.0)
VERSION ?= dev
LDFLAGS=-ldflags="-s -w -X main.Version=$(VERSION)"

# Go parameters
GOCMD=go
GOBUILD=$(GOCMD) build $(LDFLAGS)
GOCLEAN=$(GOCMD) clean
GOGET=$(GOCMD) get
GOMOD=$(GOCMD) mod

# CGO is required for systray
export CGO_ENABLED=1

all: deps build

deps:
	$(GOMOD) download
	$(GOMOD) tidy

build:
	mkdir -p $(DIST)
	$(GOBUILD) -o $(DIST)/$(BINARY) ./cmd/twitch-tray

build-linux:
	mkdir -p $(DIST)
	GOOS=linux GOARCH=amd64 $(GOBUILD) -o $(DIST)/$(BINARY)-linux-amd64 ./cmd/twitch-tray

build-darwin:
	mkdir -p $(DIST)
	GOOS=darwin GOARCH=amd64 $(GOBUILD) -o $(DIST)/$(BINARY)-darwin-amd64 ./cmd/twitch-tray
	GOOS=darwin GOARCH=arm64 $(GOBUILD) -o $(DIST)/$(BINARY)-darwin-arm64 ./cmd/twitch-tray

build-windows:
	mkdir -p $(DIST)
	GOOS=windows GOARCH=amd64 $(GOCMD) build -ldflags="-s -w -H=windowsgui -X main.Version=$(VERSION)" -o $(DIST)/$(BINARY)-windows-amd64.exe ./cmd/twitch-tray

build-all: build-linux build-darwin build-windows

run: build
	./$(DIST)/$(BINARY)

clean:
	$(GOCLEAN)
	rm -rf $(DIST)

lint:
	$(GOCMD) vet ./...
	staticcheck ./...

test:
	$(GOCMD) test -v -race ./...

# Install dependencies for development
install-deps:
	# Linux: apt-get install gcc libgtk-3-dev libayatana-appindicator3-dev
	# macOS: xcode-select --install
	# Windows: Install MinGW or TDM-GCC
	@echo "See comments in Makefile for platform-specific dependencies"
