#!/bin/bash
set -e

REPO="https://github.com/Arshdeep54/memtop"
BIN_DIR="${HOME}/.local/bin"
mkdir -p "$BIN_DIR"

OS=$(uname -s)
ARCH=$(uname -m)

case "$OS:$ARCH" in
  Linux:x86_64)
    TARGET="x86_64-unknown-linux-gnu"
    ;;
  Linux:aarch64)
    TARGET="aarch64-unknown-linux-gnu"
    ;;
  *)
    echo "error: memtop requires Linux (x86_64 or aarch64); you have $OS:$ARCH"
    exit 1
    ;;
esac

LATEST=$(curl -s "$REPO/releases/latest" | grep -oP '"tag_name": "\K[^"]+' || echo "")
if [ -z "$LATEST" ]; then
  echo "error: could not fetch latest release"
  exit 1
fi

RELEASE_URL="$REPO/releases/download/$LATEST/memtop-$TARGET"
echo "Installing memtop $LATEST from $RELEASE_URL"

curl -fL "$RELEASE_URL" -o "$BIN_DIR/memtop"
chmod +x "$BIN_DIR/memtop"

if ! echo "$PATH" | grep -q "$BIN_DIR"; then
  echo ""
  echo "warning: $BIN_DIR is not in your PATH"
  echo "add this to your shell rc (~/.bashrc, ~/.zshrc, etc):"
  echo "  export PATH=\"$BIN_DIR:\$PATH\""
  echo ""
fi

echo "✓ memtop installed to $BIN_DIR/memtop"
echo "  run: memtop --help"
