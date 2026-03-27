.PHONY: all build build-kde dev run run-kde clean lint lint-kde test test-plasmoid test-all install-plasmoid version

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
	QT_QPA_PLATFORM=offscreen /usr/lib/qt6/bin/qmltestrunner -input crates/twitch-kde/plasmoid/contents/tests -import crates/twitch-kde/plasmoid/contents

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

# Bump version: make version BUMP=patch|minor|major
version:
ifndef BUMP
	$(error Usage: make version BUMP=patch|minor|major)
endif
	@CURRENT=$$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/'); \
	IFS='.' read -r MAJOR MINOR PATCH <<< "$$CURRENT"; \
	case "$(BUMP)" in \
		major) MAJOR=$$((MAJOR + 1)); MINOR=0; PATCH=0;; \
		minor) MINOR=$$((MINOR + 1)); PATCH=0;; \
		patch) PATCH=$$((PATCH + 1));; \
		*) echo "ERROR: BUMP must be major, minor, or patch"; exit 1;; \
	esac; \
	NEW="$$MAJOR.$$MINOR.$$PATCH"; \
	sed -i "s/^version = \"$$CURRENT\"/version = \"$$NEW\"/" Cargo.toml; \
	for f in crates/twitch-app-tauri/tauri.conf.json crates/twitch-kde/tauri.conf.json; do \
		sed -i "s/\"version\": \"$$CURRENT\"/\"version\": \"$$NEW\"/" "$$f"; \
	done; \
	echo "Version: $$CURRENT -> $$NEW"; \
	echo ""; \
	echo "Files updated:"; \
	echo "  Cargo.toml (workspace.package.version)"; \
	echo "  crates/twitch-app-tauri/tauri.conf.json"; \
	echo "  crates/twitch-kde/tauri.conf.json"; \
	echo ""; \
	git add Cargo.toml crates/twitch-app-tauri/tauri.conf.json crates/twitch-kde/tauri.conf.json; \
	git commit -m "Release v$$NEW"; \
	git tag "v$$NEW"; \
	echo ""; \
	echo "Tagged v$$NEW. Push with: git push && git push --tags"

# Install plasmoid to local KDE (development)
install-plasmoid:
	kpackagetool6 --type Plasma/Applet --install crates/twitch-kde/plasmoid 2>/dev/null || \
		kpackagetool6 --type Plasma/Applet --upgrade crates/twitch-kde/plasmoid
	@# Remove from knownItems so system tray re-discovers it with EnabledByDefault on next restart
	@APPLETS_RC=~/.config/plasma-org.kde.plasma.desktop-appletsrc; \
	if [ -f "$$APPLETS_RC" ]; then \
		sed -i 's/,info.sdufresne.TwitchTray//g; s/info.sdufresne.TwitchTray,//g; s/info.sdufresne.TwitchTray//g' "$$APPLETS_RC"; \
	fi
