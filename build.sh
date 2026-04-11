#!/usr/bin/env bash
# build.sh — Build AI Quant Terminal from scratch
#
# Usage:
#   bash build.sh              # full production build
#   bash build.sh --dev        # install deps only, then launch dev server
#   bash build.sh --deps-only  # install all deps without building
#
# Prerequisites that this script installs automatically:
#   • npm packages (Node.js ≥ 18 must be on PATH)
#   • Tauri CLI  (cargo install tauri-cli, requires Rust ≥ 1.77)
#   • Python packages (Python ≥ 3.9 must be on PATH)
#
# If Node.js or Rust is missing the script prints instructions and exits.

set -euo pipefail

# ── Colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'   # reset

info()    { echo -e "${CYAN}[build]${NC} $*"; }
ok()      { echo -e "${GREEN}[build] ✓${NC} $*"; }
warn()    { echo -e "${YELLOW}[build] ⚠${NC} $*"; }
die()     { echo -e "${RED}[build] ✗ $*${NC}" >&2; exit 1; }

# ── Parse flags ──────────────────────────────────────────────────────────────
MODE="build"          # build | dev | deps-only
for arg in "$@"; do
  case "$arg" in
    --dev)       MODE="dev"       ;;
    --deps-only) MODE="deps-only" ;;
    --help|-h)
      echo "Usage: bash build.sh [--dev | --deps-only]"
      echo "  (no flag)    Install deps + build production Tauri app"
      echo "  --dev        Install deps + launch dev server (hot-reload)"
      echo "  --deps-only  Install deps only (no build)"
      exit 0
      ;;
    *) die "Unknown option: $arg.  Run  bash build.sh --help  for usage." ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

echo ""
echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
echo -e "${BOLD}║    AI Quant Terminal — build.sh      ║${NC}"
echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"
echo ""

# ── Step 1: Check Node.js ────────────────────────────────────────────────────
info "Checking Node.js …"
if ! command -v node &>/dev/null; then
  die "Node.js not found.  Install it from https://nodejs.org/ (version ≥ 18)"
fi
NODE_VER=$(node --version)
NODE_MAJOR=$(echo "$NODE_VER" | sed 's/v\([0-9]*\).*/\1/')
if [ "$NODE_MAJOR" -lt 18 ]; then
  die "Node.js $NODE_VER is too old.  Version ≥ 18 is required."
fi
ok "Node.js $NODE_VER"

# ── Step 2: Check Python ─────────────────────────────────────────────────────
info "Checking Python …"
PYTHON_CMD=""
for py in python3 python; do
  if command -v "$py" &>/dev/null; then
    PY_VER=$("$py" --version 2>&1 | awk '{print $2}')
    PY_MAJOR=$(echo "$PY_VER" | cut -d. -f1)
    PY_MINOR=$(echo "$PY_VER" | cut -d. -f2)
    if [ "$PY_MAJOR" -ge 3 ] && [ "$PY_MINOR" -ge 9 ]; then
      PYTHON_CMD="$py"
      break
    fi
  fi
done
if [ -z "$PYTHON_CMD" ]; then
  die "Python ≥ 3.9 not found.  Install it from https://www.python.org/"
fi
ok "Python $($PYTHON_CMD --version)"

# ── Step 3: Check Rust ───────────────────────────────────────────────────────
info "Checking Rust …"
if ! command -v cargo &>/dev/null; then
  echo -e "${YELLOW}[build]${NC} Rust / cargo not found."
  echo "  Install rustup (the Rust toolchain manager):"
  echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  echo "  Then re-run this script."
  exit 1
fi
ok "Rust $(rustc --version)"

# ── Step 4: Install npm packages ─────────────────────────────────────────────
info "Installing npm packages …"
npm install --silent
ok "npm packages installed"

# ── Step 5: Install Tauri CLI ─────────────────────────────────────────────────
info "Checking Tauri CLI …"
if ! cargo tauri --version &>/dev/null 2>&1; then
  info "Tauri CLI not found — installing (this takes a few minutes) …"
  cargo install tauri-cli --version "^2" --locked
  ok "Tauri CLI installed"
else
  ok "Tauri CLI $(cargo tauri --version)"
fi

# ── Step 6: Install Python packages ──────────────────────────────────────────
info "Installing Python packages …"
"$PYTHON_CMD" -m pip install --quiet --upgrade pip
"$PYTHON_CMD" -m pip install --quiet \
  backtrader \
  akshare \
  "langchain>=0.2" \
  "langchain-openai>=0.1" \
  pyyaml
ok "Python packages installed"

# ── Step 7: Create config.yaml from example (if missing) ─────────────────────
info "Checking config.yaml …"
if [ -f "config.yaml" ]; then
  ok "config.yaml already exists — skipping copy"
else
  cp config.example.yaml config.yaml
  warn "Created config.yaml from config.example.yaml"
  warn "→ Open config.yaml and fill in your LLM provider and API key."
  echo ""
  echo -e "  ${BOLD}Quick start:${NC}"
  echo "    1. Open config.yaml in your editor"
  echo "    2. Set  llm.provider  (e.g. openai, kimi, glm, siliconflow)"
  echo "    3. Set  llm.api_key   (your API key for that provider)"
  echo "    4. Re-run this script (or  npm run tauri dev)"
  echo ""
fi

# ── Step 8: Build or launch ───────────────────────────────────────────────────
case "$MODE" in
  deps-only)
    ok "All dependencies installed.  Edit config.yaml and run:"
    echo "   npm run tauri dev    # development"
    echo "   npm run tauri build  # production"
    ;;
  dev)
    info "Launching dev server …"
    echo ""
    exec npm run tauri dev
    ;;
  build)
    info "Building production Tauri app …"
    npm run tauri build
    echo ""
    ok "Build complete!  Installer is in src-tauri/target/release/bundle/"
    ;;
esac
