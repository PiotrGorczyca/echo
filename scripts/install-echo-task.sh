#!/usr/bin/env bash
#
# Install echo-task CLI tool to user's PATH
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ECHO_TASK_SCRIPT="$SCRIPT_DIR/echo-task"

# Default installation directory
INSTALL_DIR="${HOME}/.local/bin"

echo "Installing echo-task CLI tool..."

# Create install directory if it doesn't exist
mkdir -p "$INSTALL_DIR"

# Copy the script
cp "$ECHO_TASK_SCRIPT" "$INSTALL_DIR/echo-task"
chmod +x "$INSTALL_DIR/echo-task"

echo "✓ Installed echo-task to $INSTALL_DIR/echo-task"

# Check if INSTALL_DIR is in PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    echo "⚠ $INSTALL_DIR is not in your PATH."
    echo ""
    echo "Add this line to your shell config (~/.bashrc, ~/.zshrc, etc.):"
    echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
    echo ""
    echo "Then restart your shell or run: source ~/.bashrc (or ~/.zshrc)"
else
    echo "✓ $INSTALL_DIR is already in your PATH"
fi

echo ""
echo "Usage:"
echo "  echo-task status         - Check API status"
echo "  echo-task list           - List tasks"
echo "  echo-task create <title> - Create a task"
echo "  echo-task done <id>      - Mark task as done"
echo "  echo-task help           - Show all commands"





