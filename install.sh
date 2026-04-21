#!/usr/bin/env sh
# AgentScribe installer
# Usage: curl -sSf https://raw.githubusercontent.com/coding/AgentScribe/main/install.sh | sh
set -e

REPO="coding/AgentScribe"
BINARY="agentscribe"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS
OS="$(uname -s)"
case "$OS" in
  Linux)  OS="linux" ;;
  Darwin) OS="macos" ;;
  *)
    echo "Unsupported operating system: $OS" >&2
    exit 1
    ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *)
    echo "Unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

ASSET="${BINARY}-${OS}-${ARCH}"

# Resolve latest release tag
echo "Fetching latest AgentScribe release..."
TAG="$(curl -sSf "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' \
  | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"

if [ -z "$TAG" ]; then
  echo "Could not determine latest release tag." >&2
  exit 1
fi

echo "Installing AgentScribe ${TAG} (${OS}/${ARCH})..."

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}.tar.gz"
CHECKSUM_URL="${DOWNLOAD_URL}.sha256"

# Create a temporary directory
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# Download tarball and checksum
curl -sSfL "$DOWNLOAD_URL" -o "$TMP_DIR/${ASSET}.tar.gz"
curl -sSfL "$CHECKSUM_URL" -o "$TMP_DIR/${ASSET}.tar.gz.sha256"

# Verify checksum
cd "$TMP_DIR"
if command -v sha256sum > /dev/null 2>&1; then
  sha256sum -c "${ASSET}.tar.gz.sha256"
elif command -v shasum > /dev/null 2>&1; then
  shasum -a 256 -c "${ASSET}.tar.gz.sha256"
else
  echo "Warning: could not verify checksum (no sha256sum or shasum found)" >&2
fi

# Extract
tar -xzf "${ASSET}.tar.gz"

# Install binary
mkdir -p "$INSTALL_DIR"
install -m 755 "$BINARY" "$INSTALL_DIR/$BINARY"

echo "Installed $BINARY to $INSTALL_DIR/$BINARY"

# Install shell completions (optional)
install_completions() {
  SHELL_NAME="$(basename "$SHELL")"
  case "$SHELL_NAME" in
    bash)
      COMPLETION_DIR="${BASH_COMPLETION_USER_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions}"
      mkdir -p "$COMPLETION_DIR"
      cp completions/agentscribe.bash "$COMPLETION_DIR/agentscribe"
      echo "Installed bash completion to $COMPLETION_DIR/agentscribe"
      ;;
    zsh)
      COMPLETION_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/zsh/site-functions"
      mkdir -p "$COMPLETION_DIR"
      cp completions/_agentscribe "$COMPLETION_DIR/_agentscribe"
      echo "Installed zsh completion to $COMPLETION_DIR/_agentscribe"
      ;;
    fish)
      COMPLETION_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/fish/completions"
      mkdir -p "$COMPLETION_DIR"
      cp completions/agentscribe.fish "$COMPLETION_DIR/agentscribe.fish"
      echo "Installed fish completion to $COMPLETION_DIR/agentscribe.fish"
      ;;
    *)
      echo "Shell completions not installed (unsupported shell: $SHELL_NAME)."
      echo "Run 'agentscribe completions bash|zsh|fish' to generate them manually."
      ;;
  esac
}

if [ -d "$TMP_DIR/completions" ]; then
  install_completions
fi

# Check if INSTALL_DIR is in PATH
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "Note: $INSTALL_DIR is not in your PATH."
    echo "Add the following to your shell profile:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

echo ""
echo "AgentScribe ${TAG} installed successfully!"
echo "Run 'agentscribe --help' to get started."
