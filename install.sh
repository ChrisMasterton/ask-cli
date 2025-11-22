#!/bin/bash

# Install script for ask CLI tool

set -e

echo "🔨 Building release version..."
cargo build --release

echo "📦 Installing to /usr/local/bin..."
echo "This will require sudo permissions."

# Check if ask already exists and backup
if [ -f /usr/local/bin/ask ]; then
    echo "⚠️  Existing 'ask' found at /usr/local/bin/ask"
    echo "Creating backup at /usr/local/bin/ask.backup"
    sudo cp /usr/local/bin/ask /usr/local/bin/ask.backup
fi

# Copy the new binary
sudo cp target/release/ask /usr/local/bin/ask

# Make sure it's executable
sudo chmod +x /usr/local/bin/ask

echo "✅ Installation complete!"
echo ""
echo "You can now use 'ask' from anywhere in your terminal."
echo "Try: ask --help"
echo ""
echo "New features in this version:"
echo "  • Skip commands with 's' (continue to next command)"
echo "  • Insert custom commands with 'i' (run something first, then return to original)"