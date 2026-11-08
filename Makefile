.PHONY: build release test test-live install uninstall clean help

# Default target
help:
	@echo "Available targets:"
	@echo "  make build      - Build debug version"
	@echo "  make release    - Build optimized release version"
	@echo "  make test       - Run deterministic tests"
	@echo "  make test-live  - Run live OpenRouter contract tests"
	@echo "  make install    - Build release and install to /usr/local/bin"
	@echo "  make uninstall  - Remove from /usr/local/bin"
	@echo "  make clean      - Clean build artifacts"
	@echo "  make help       - Show this help message"

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

test-live:
	cargo test -- --ignored --show-output --test-threads=4

install: release
	@bash scripts/install-lifecycle.sh install make

uninstall:
	@bash scripts/install-lifecycle.sh uninstall make

clean:
	cargo clean
