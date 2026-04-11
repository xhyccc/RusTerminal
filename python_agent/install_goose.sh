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
echo "  1. Choose your LLM provider and export the shared config vars, e.g.:"
echo ""
echo "     # OpenAI (default)"
echo "       export LLM_PROVIDER=openai"
echo "       export LLM_API_KEY=sk-..."
echo ""
echo "     # Kimi (Moonshot)"
echo "       export LLM_PROVIDER=kimi"
echo "       export LLM_API_KEY=<moonshot-api-key>"
echo ""
echo "     # GLM (Zhipu AI)"
echo "       export LLM_PROVIDER=glm"
echo "       export LLM_API_KEY=<zhipu-api-key>"
echo ""
echo "     # SiliconFlow"
echo "       export LLM_PROVIDER=siliconflow"
echo "       export LLM_API_KEY=<siliconflow-api-key>"
echo ""
echo "     # Azure OpenAI"
echo "       export LLM_PROVIDER=azure"
echo "       export AZURE_OPENAI_API_KEY=<key>"
echo "       export AZURE_OPENAI_ENDPOINT=https://<resource>.openai.azure.com/"
echo "       export AZURE_OPENAI_DEPLOYMENT=<deployment-name>"
echo ""
echo "     # Optional overrides (any provider)"
echo "       export LLM_MODEL=<model-name>     # override default model"
echo "       export LLM_BASE_URL=<url>          # override API base URL"
echo ""
echo "  2. Run the quant terminal and start experimenting!"
