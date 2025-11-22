.PHONY: build release install uninstall clean help

# Default target
help:
	@echo "Available targets:"
	@echo "  make build      - Build debug version"
	@echo "  make release    - Build optimized release version"
	@echo "  make install    - Build release and install to /usr/local/bin"
	@echo "  make uninstall  - Remove from /usr/local/bin"
	@echo "  make clean      - Clean build artifacts"
	@echo "  make help       - Show this help message"

build:
	cargo build

release:
	cargo build --release

install: release
	@echo "Installing to /usr/local/bin (requires sudo)..."
	@if [ -f /usr/local/bin/ask ]; then \
		echo "Backing up existing binary to /usr/local/bin/ask.backup"; \
		sudo cp /usr/local/bin/ask /usr/local/bin/ask.backup; \
	fi
	sudo cp target/release/ask /usr/local/bin/ask
	sudo chmod +x /usr/local/bin/ask
	@echo "✅ Installed successfully!"
	@echo "Run 'ask --help' to see the new options"

uninstall:
	@echo "Removing ask from /usr/local/bin (requires sudo)..."
	sudo rm -f /usr/local/bin/ask
	@if [ -f /usr/local/bin/ask.backup ]; then \
		echo "Restoring backup..."; \
		sudo mv /usr/local/bin/ask.backup /usr/local/bin/ask; \
	fi
	@echo "✅ Uninstalled successfully!"

clean:
	cargo clean