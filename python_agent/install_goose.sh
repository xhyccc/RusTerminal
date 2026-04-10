#!/usr/bin/env bash
# install_goose.sh — Install the goose CLI agent
# https://github.com/aaif-goose/goose
#
# Usage:
#   bash python_agent/install_goose.sh
#
# After installation, restart your terminal (or `source ~/.bashrc`) so that
# `goose` is available on PATH.

set -euo pipefail

INSTALL_SCRIPT_URL="https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh"

echo "[install_goose] Downloading and running the official goose install script…"
curl -fsSL "$INSTALL_SCRIPT_URL" | bash

echo ""
echo "[install_goose] ✓ goose installed."
echo "[install_goose]   You may need to restart your shell or run:"
echo "                    source \$HOME/.bashrc   (bash)"
echo "                    source \$HOME/.zshrc    (zsh)"
echo ""
echo "[install_goose] Verify with:  goose --version"
echo ""
echo "[install_goose] Next steps:"
echo "  1. Configure your provider (e.g. OpenAI):"
echo "       export GOOSE_PROVIDER=openai"
echo "       export OPENAI_API_KEY=sk-..."
echo "  2. Run the quant terminal and start experimenting!"
