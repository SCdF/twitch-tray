.PHONY: all build dev run clean lint test install-deps

# Build directory
DIST=dist

all: build

# Install dependencies
deps:
	cargo fetch

# Development build
build:
	cd crates/twitch-app-tauri && cargo build

# Release build
release:
	cd crates/twitch-app-tauri && cargo build --release

# Development with hot reload
dev:
	cd crates/twitch-app-tauri && cargo tauri dev

# Run the built binary
run: build
	./crates/twitch-app-tauri/target/debug/twitch-tray

# Clean build artifacts
clean:
	cargo clean
	rm -rf $(DIST)

# Run lints
lint:
	cargo fmt --check
	cargo clippy --workspace -- -D warnings

# Run tests
test:
	cargo test --workspace

# Format code
fmt:
	cargo fmt

# Build for distribution (uses Tauri bundler)
dist:
	cd crates/twitch-app-tauri && cargo tauri build

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
