.PHONY: all build dev run clean lint test install-deps

# Build directory
DIST=dist

all: build

# Install dependencies
deps:
	cd src-tauri && cargo fetch

# Development build
build:
	cd src-tauri && cargo build

# Release build
release:
	cd src-tauri && cargo build --release

# Development with hot reload
dev:
	cd src-tauri && cargo tauri dev

# Run the built binary
run: build
	./src-tauri/target/debug/twitch-tray

# Clean build artifacts
clean:
	cd src-tauri && cargo clean
	rm -rf $(DIST)

# Run lints
lint:
	cd src-tauri && cargo fmt --check
	cd src-tauri && cargo clippy -- -D warnings

# Run tests
test:
	cd src-tauri && cargo test

# Format code
fmt:
	cd src-tauri && cargo fmt

# Build for distribution (uses Tauri bundler)
dist:
	cd src-tauri && cargo tauri build

# Install platform-specific dependencies
install-deps:
	@echo "Platform-specific dependencies:"
	@echo ""
	@echo "Linux (Debian/Ubuntu):"
	@echo "  sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev"
	@echo ""
	@echo "macOS:"
	@echo "  xcode-select --install"
	@echo ""
	@echo "Windows:"
	@echo "  Install Visual Studio Build Tools with C++ workload"
