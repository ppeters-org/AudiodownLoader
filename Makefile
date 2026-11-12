TARGET = AudioDownloader
APP_NAME = AudioDownloader

.PHONY: all build run clean test dmg install init help assets

# Default target
all: build

# Initialize Cargo.toml file
init:
	@if [ ! -f Cargo.toml ]; then \
		echo "[package]" > Cargo.toml; \
		echo "name = \"$(TARGET)\"" >> Cargo.toml; \
		echo "version = \"0.1.0\"" >> Cargo.toml; \
		echo "edition = \"2024\"" >> Cargo.toml; \
		echo "" >> Cargo.toml; \
		echo "[package.metadata.bundle]" >> Cargo.toml; \
		echo "name = \"$(TARGET)\"" >> Cargo.toml; \
		echo "identifier = \"com.$(TARGET).app\"" >> Cargo.toml; \
		echo "icon = []" >> Cargo.toml; \
		echo "category = \"Utility\"" >> Cargo.toml; \
		echo "short_description = \"$(APP_NAME) is a GUI app that does something\"" >> Cargo.toml; \
		echo "long_description = \"$(APP_NAME) is a GUI app that does something more.\"" >> Cargo.toml; \
		echo "" >> Cargo.toml; \
		echo "[dependencies]" >> Cargo.toml; \
		echo "eframe = \"0.31\"" >> Cargo.toml; \
		echo "rfd = \"0.15\"" >> Cargo.toml; \
		echo "arboard = { version = \"3\", features = [\"wayland-data-control\"] }" >> Cargo.toml; \
		echo "clap = { version = \"4\", features = [\"derive\"] }" >> Cargo.toml; \
		echo "serde = { version = \"1\", features = [\"derive\"] }" >> Cargo.toml; \
		echo "serde_json = \"1\"" >> Cargo.toml; \
		echo "directories = \"5\"" >> Cargo.toml; \
		echo "Cargo.toml created successfully"; \
		echo "You probably need to change the short_description and long_descriptions"; \
	else \
		echo "Cargo.toml already exists"; \
	fi

# Build the project in release mode
build:
	cargo build --release

# Run the project
run:
	cargo run

# Clean build artifacts
clean:
	cargo clean

# Run tests
test:
	cargo test

# Create macOS .dmg distribution
dmg:
	@command -v cargo-bundle >/dev/null 2>&1 || \
		(echo "cargo-bundle not found. Installing..." && \
		cargo install cargo-bundle)
	cargo bundle --release
	@echo "DMG created at: target/release/bundle/osx/$(APP_NAME).app"

# Install the app to /Applications (macOS only)
install: build
	@command -v cargo-bundle >/dev/null 2>&1 || \
		(echo "cargo-bundle not found. Installing..." && \
		cargo install cargo-bundle)
	cargo bundle --release
	@if [ -d "target/release/bundle/osx/$(APP_NAME).app" ]; then \
		echo "Installing $(APP_NAME).app to /Applications..."; \
		cp -R "target/release/bundle/osx/$(APP_NAME).app" /Applications/; \
		echo "Installation complete! You can now find $(APP_NAME) in your Applications folder."; \
		echo "Make sure the assests (Top2000 excel and Radio2 logo) are in /Applications/Top2026.app/Contents/MacOS/assets/."; \
	else \
		echo "Error: Application bundle not found. Run 'make dmg' first."; \
	fi

assets:
	@echo "Copying assets to /Applications/Top2026.app/Contents/MacOS/assets/"
	@cp -r "assets" "/Applications/Top2026.app/Contents/MacOS/assets/"

# Show help
help:
	@echo "$(APP_NAME) - Makefile"
	@echo ""
	@echo "Available targets:"
	@echo "  make init    - Initialize Cargo.toml file"
	@echo "  make build   - Build the project in release mode"
	@echo "  make run     - Run the project"
	@echo "  make clean   - Clean build artifacts"
	@echo "  make test    - Run tests"
	@echo "  make dmg     - Create macOS .dmg distribution"
	@echo "  make install - Install app to /Applications (macOS only)"
	@echo "  make assets  - Copy assets to app bundle (macOS only)"
	@echo "  make help    - Show this help message"
