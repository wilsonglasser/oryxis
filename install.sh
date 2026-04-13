#!/bin/bash
set -e

echo "Building Oryxis release..."
cargo build --release

echo "Installing binaries..."
sudo cp target/release/oryxis /usr/local/bin/
sudo cp target/release/oryxis-mcp /usr/local/bin/

echo "Installing icon..."
sudo mkdir -p /usr/share/icons/hicolor/64x64/apps
sudo cp resources/logo_64.png /usr/share/icons/hicolor/64x64/apps/oryxis.png

echo "Installing desktop entry..."
sudo cp resources/oryxis.desktop /usr/share/applications/

echo "Done! Run 'oryxis' to start."
