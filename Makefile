.PHONY: all build build-kde dev run run-kde clean lint lint-kde test test-plasmoid test-all install-plasmoid

# Build directory
DIST=dist

all: build

# Install dependencies
deps:
	cargo fetch

# Development build (Tauri tray target)
build:
	cd crates/twitch-app-tauri && cargo build

# Build KDE daemon
build-kde:
	cd crates/twitch-kde && cargo build

# Release build
release:
	cd crates/twitch-app-tauri && cargo build --release

# Release build (KDE daemon)
release-kde:
	cd crates/twitch-kde && cargo build --release

# Development with hot reload
dev:
	cd crates/twitch-app-tauri && cargo tauri dev

# Run the Tauri tray binary
run: build
	./target/debug/twitch-tray

# Run the KDE daemon
run-kde: build-kde
	./target/debug/twitch-kde

# Clean build artifacts
clean:
	cargo clean
	rm -rf $(DIST)

# Run lints
lint:
	cargo fmt --check
	cargo clippy --workspace -- -D warnings

# Run lints for KDE crate only
lint-kde:
	cargo clippy -p twitch-kde -- -D warnings

# Run Rust tests
test:
	cargo test --workspace

# Run QML plasmoid tests
test-plasmoid:
	@command -v /usr/lib/qt6/bin/qmltestrunner >/dev/null 2>&1 || { \
		echo "ERROR: qmltestrunner not found."; \
		echo "Install: qt6-declarative-dev (Debian/Ubuntu) or qt6-declarative (Arch)"; \
		exit 1; \
	}
	export QT_QPA_PLATFORM=offscreen
	/usr/lib/qt6/bin/qmltestrunner -input crates/twitch-kde/plasmoid/contents/tests -import crates/twitch-kde/plasmoid/contents

# Run all tests (Rust + QML)
test-all: test test-plasmoid

# Format code
fmt:
	cargo fmt

# Build for distribution (uses Tauri bundler)
dist:
	cd crates/twitch-app-tauri && cargo tauri build

# Build KDE plasmoid package for installation
dist-kde: release-kde
	@mkdir -p $(DIST)
	@rm -rf $(DIST)/twitch-kde-plasmoid
	@mkdir -p $(DIST)/twitch-kde-plasmoid/contents/ui
	cp crates/twitch-kde/plasmoid/metadata.json $(DIST)/twitch-kde-plasmoid/
	cp crates/twitch-kde/plasmoid/contents/ui/*.qml crates/twitch-kde/plasmoid/contents/ui/qmldir $(DIST)/twitch-kde-plasmoid/contents/ui/
	cp target/release/twitch-kde $(DIST)/twitch-kde
	@echo ""
	@echo "KDE plasmoid package: $(DIST)/twitch-kde-plasmoid/"
	@echo "KDE daemon binary:    $(DIST)/twitch-kde"
	@echo ""
	@echo "Install plasmoid:  kpackagetool6 --type Plasma/Applet --install $(DIST)/twitch-kde-plasmoid"
	@echo "Install daemon:    sudo cp $(DIST)/twitch-kde /usr/bin/twitch-kde"

# Install plasmoid to local KDE (development)
install-plasmoid:
	kpackagetool6 --type Plasma/Applet --install crates/twitch-kde/plasmoid 2>/dev/null || \
		kpackagetool6 --type Plasma/Applet --upgrade crates/twitch-kde/plasmoid
