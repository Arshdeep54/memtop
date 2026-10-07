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

# /releases/latest redirects to /releases/tag/<tag>
LATEST=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$REPO/releases/latest" | grep -oP '/tag/\K.+' || echo "")
if [ -n "$LATEST" ]; then
  RELEASE_URL="$REPO/releases/download/$LATEST/memtop-$TARGET"
  if curl -fL --max-time 5 "$RELEASE_URL" -o "$BIN_DIR/memtop" 2>/dev/null; then
    chmod +x "$BIN_DIR/memtop"
    echo "✓ memtop $LATEST installed from release"
  else
    echo "note: pre-built binary unavailable, building from source..."
    cargo install --git "$REPO" --force
    exit 0
  fi
else
  echo "note: no releases found, installing via cargo..."
  if ! command -v cargo &> /dev/null; then
    echo "error: cargo not found. install Rust: https://rustup.rs/"
    exit 1
  fi
  cargo install --git "$REPO"
  exit 0
fi

if ! echo "$PATH" | grep -q "$BIN_DIR"; then
  echo "warning: $BIN_DIR is not in your PATH"
  echo "add to your shell rc: export PATH=\"$BIN_DIR:\$PATH\""
fi

echo "✓ memtop ready"
memtop --help | head -3
