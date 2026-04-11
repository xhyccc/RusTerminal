# ⬡ AI Quant Terminal

> **AI-powered quantitative strategy research desktop app for Chinese A-share markets**
> Built with Rust (Tauri) + Vue 3 — runs entirely on your local machine.

---

## Table of Contents

1. [Overview](#overview)
2. [Feature Highlights](#feature-highlights)
3. [Architecture](#architecture)
4. [Tech Stack](#tech-stack)
5. [Project Structure](#project-structure)
6. [Getting Started](#getting-started)
   - [Prerequisites](#prerequisites)
   - [Installation](#installation)
   - [Running in Development](#running-in-development)
   - [Building for Production](#building-for-production)
7. [Tutorial: Generate Your First Strategy](#tutorial-generate-your-first-strategy)
8. [UI Walkthrough](#ui-walkthrough)
9. [Design Details](#design-details)
   - [Rust Backend](#rust-backend)
   - [AI Agent Pipeline](#ai-agent-pipeline)
   - [Frontend Components](#frontend-components)
   - [Data Flow](#data-flow)
10. [Configuration](#configuration)
11. [Environment Variables](#environment-variables)
    - [Shared config](#shared-config)
    - [Provider-specific API key variables](#provider-specific-api-key-variables)
    - [Azure OpenAI](#azure-openai)
    - [Provider defaults](#provider-defaults)

---

## Overview

**AI Quant Terminal** lets you describe a trading idea in plain language and instantly get:

- a fully working **Backtrader strategy** written and executed by an AI agent,
- a real-time **equity-curve chart** with key performance metrics (Sharpe, max-drawdown, total return),
- a **strategy monitor dashboard** where you can enable / disable any saved strategy and receive desktop notifications.

Everything runs locally — no cloud service required. The AI agent uses [goose](https://block.github.io/goose/) (a local Rust-based AI agent) with an automatic fallback to a Python / LangChain + OpenAI pipeline when goose is not installed.

---

## Feature Highlights

| Feature | Description |
|---|---|
| 🤖 **Dual AI engines** | Goose recipe (primary) or LangChain fallback; both share the same LLM config |
| 🔌 **Multi-provider LLM** | OpenAI, Kimi, GLM, SiliconFlow, Azure OpenAI — switch with one env var |
| 🔁 **Self-healing agent** | Up to 5 automatic retry-and-fix cycles on script errors |
| 📈 **Live equity chart** | ECharts area chart updated immediately after each successful backtest |
| 📊 **Performance metrics** | Sharpe ratio, max drawdown %, total return %, number of trades |
| 📡 **Strategy monitor** | Persistent JSON store; enable/disable per-strategy toggle |
| 🔔 **Desktop notifications** | Native OS alerts fired by the Rust market engine every 60 s |
| 🖥️ **Terminal emulator** | Embedded xterm.js pane with ANSI colour-coded agent logs |
| ⚡ **60 FPS UI** | Tauri WebView — native performance, ~8 MB binary footprint |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        AI Quant Terminal                            │
│                                                                     │
│  ┌──────────────── Vue 3 Frontend (WebView) ─────────────────────┐  │
│  │                                                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐ │  │
│  │  │StrategyInput│  │ AgentTerminal│  │    BacktestChart      │ │  │
│  │  │             │  │  (xterm.js)  │  │  (ECharts equity      │ │  │
│  │  │ [idea text] │  │              │  │   curve + metrics)    │ │  │
│  │  │ [▶ 开始研发]│  │ [AGENT_LOG]  │  │                      │ │  │
│  │  └──────┬──────┘  └──────▲───────┘  └──────────────────────┘ │  │
│  │         │  invoke        │ event                               │  │
│  │         │  run_agent     │ agent-log                           │  │
│  │  ┌──────▼──────────────────────────────────────────────────┐  │  │
│  │  │                   MonitorPanel                           │  │  │
│  │  │  strategy cards ── toggle ── metrics mini-grid           │  │  │
│  │  └─────────────────────────────────────────────────────────┘  │  │
│  └───────────────────────────┬────────────────────────────────────┘  │
│                  Tauri IPC (invoke / emit)                           │
│  ┌────────────────────────── Rust Core ──────────────────────────┐  │
│  │                                                                │  │
│  │  ┌──────────────────┐   ┌──────────────────────────────────┐  │  │
│  │  │  agent_runner.rs │   │       market_engine.rs           │  │  │
│  │  │                  │   │                                  │  │  │
│  │  │ find_goose()     │   │ • Tokio interval (60 s)          │  │  │
│  │  │   ↓              │   │ • reads strategies.json          │  │  │
│  │  │ run_with_goose() │   │ • emits market-heartbeat event   │  │  │
│  │  │   ↓ (fallback)   │   │ • calls notifier on first trigger│  │  │
│  │  │ run_with_python()│   └──────────────┬───────────────────┘  │  │
│  │  │                  │                  │                       │  │
│  │  │ streams stdout → │          ┌───────▼──────────┐           │  │
│  │  │ "agent-log" event│          │   notifier.rs    │           │  │
│  │  └────────┬─────────┘          │ notify-rust crate│           │  │
│  │           │                    └──────────────────┘           │  │
│  └───────────┼────────────────────────────────────────────────────┘  │
│              │ spawn subprocess                                       │
│  ┌───────────▼──────────────────────────────────────────────────┐   │
│  │                  Python Agent Layer                           │   │
│  │                                                               │   │
│  │  agent_loop.py ──► goose run --recipe quant_strategy.yaml    │   │
  │                    (env vars: LLM_PROVIDER, LLM_API_KEY …)   │   │
│  │                         │                                     │   │
│  │                    ┌────▼────────────────────────────┐        │   │
│  │                    │  goose (Rust AI agent)           │        │   │
│  │                    │  quant_strategy.yaml recipe      │        │   │
│  │                    │  ┌─────────────────────────────┐│        │   │
│  │                    │  │ STEP 1: generate strategy   ││        │   │
│  │                    │  │ STEP 2: run backtest        ││        │   │
│  │                    │  │ STEP 3: evaluate            ││        │   │
│  │                    │  │ STEP 4: self-heal (×5)      ││        │   │
│  │                    │  │ STEP 5: save report.json    ││        │   │
│  │                    │  └─────────────────────────────┘│        │   │
│  │                    └─────────────────────────────────┘        │   │
│  │                                                               │   │
│  │  Python fallback ──► LangChain + configurable LLM + backtrader     │   │
│  │                                                               │   │
│  │  Workspace files:                                             │   │
│  │    python_agent/workspace/temp_strategy.py  (generated code)  │   │
│  │    python_agent/workspace/report.json       (latest results)  │   │
│  │    python_agent/workspace/strategies.json   (all strategies)  │   │
│  └───────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

### Event / IPC Flow

```
User types idea
      │
      ▼
StrategyInput.vue  ──invoke("run_agent", {idea})──►  agent_runner.rs
                                                          │
                                              spawn goose / python3
                                                          │
                                              stdout line-by-line
                                                          │
                                       emit("agent-log", line) ◄──────┐
                                                          │            │
                        AgentTerminal.vue ◄──────────────┘            │
                        (xterm.js renders coloured output)             │
                                                                       │
                        on "[AGENT_SUCCESS]" ──────────────────────────┘
                              │
                    BacktestChart.vue reads report.json
                    MonitorPanel.vue  reads strategies.json
```

---

## Tech Stack

| Layer | Technology | Purpose |
|---|---|---|
| Desktop shell | [Tauri 2](https://tauri.app) | Rust runtime, system tray, native notifications |
| Backend logic | Rust (Tokio async) | Agent runner, market engine, notifier |
| Frontend framework | [Vue 3](https://vuejs.org) (Composition API) | Reactive UI components |
| Styling | [Tailwind CSS 3](https://tailwindcss.com) | Utility-first dark-terminal theme |
| Terminal emulator | [@xterm/xterm 5](https://xtermjs.org) | Embedded ANSI terminal pane |
| Charts | [ECharts 5](https://echarts.apache.org) | Equity-curve area chart |
| Build tool | [Vite 5](https://vitejs.dev) | Fast HMR dev server + bundler |
| Primary AI agent | [goose](https://block.github.io/goose/) | YAML-recipe AI agent (Rust) |
| Fallback AI | LangChain + configurable LLM (OpenAI / Kimi / GLM / SiliconFlow / Azure) | Python-based strategy generation |
| Backtesting | [backtrader](https://www.backtrader.com/) | Python strategy execution engine |
| Market data | [akshare](https://akshare.akfamily.xyz/) | Chinese A-share data (free) |

---

## Project Structure

```
RusTerminal/
├── src/                          # Vue 3 frontend
│   ├── main.js                   # App entry point
│   ├── App.vue                   # Root layout: sidebar + tabs + status bar
│   ├── style.css                 # Tailwind base + custom terminal colours
│   └── components/
│       ├── StrategyInput.vue     # Idea textarea + "▶ 开始研发" button
│       ├── AgentTerminal.vue     # xterm.js live log pane
│       ├── BacktestChart.vue     # ECharts equity curve + metrics grid
│       └── MonitorPanel.vue      # Strategy cards with enable/disable toggle
│
├── src-tauri/                    # Rust/Tauri backend
│   ├── src/
│   │   ├── main.rs               # Tauri entry (desktop)
│   │   ├── lib.rs                # Plugin setup + command registration
│   │   ├── agent_runner.rs       # run_agent command (goose + Python fallback)
│   │   ├── market_engine.rs      # Background 60 s heartbeat loop
│   │   └── notifier.rs           # Native desktop notification wrapper
│   ├── Cargo.toml                # Rust dependencies
│   ├── tauri.conf.json           # Window size, bundle, tray settings
│   └── capabilities/default.json # Tauri v2 permissions
│
├── python_agent/
│   ├── agent_loop.py             # CLI entry — goose first, Python fallback
│   ├── quant_strategy.yaml       # Goose recipe (5-step self-healing workflow)
│   ├── template_bt.py            # Fallback Backtrader template
│   ├── data_fetcher.py           # akshare helper utilities
│   ├── install_goose.sh          # One-liner goose installer
│   └── workspace/
│       ├── temp_strategy.py      # (generated) current strategy under test
│       ├── report.json           # (generated) latest backtest results
│       └── strategies.json       # (generated) all saved strategies
│
├── index.html                    # Vite HTML shell
├── vite.config.js                # Vite + Vue plugin config
├── tailwind.config.js            # Custom colour palette
└── package.json                  # npm dependencies
```

---

## Getting Started

### Prerequisites

| Tool | Version | Notes |
|---|---|---|
| [Node.js](https://nodejs.org/) | ≥ 18 | Frontend build |
| [Rust](https://rustup.rs/) | stable | Tauri backend |
| [Python](https://www.python.org/) | ≥ 3.9 | Agent + backtest runner |
| [goose](https://block.github.io/goose/) | latest | Primary AI agent *(optional)* |

**Install Python dependencies:**

```bash
pip install backtrader akshare langchain langchain-openai
```

**Install goose (optional but recommended):**

```bash
curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash
# or use the helper script:
bash python_agent/install_goose.sh
```

### Installation

```bash
# Clone the repository
git clone https://github.com/xhyccc/RusTerminal.git
cd RusTerminal

# Install frontend dependencies
npm install

# Install Tauri CLI (first time only)
cargo install tauri-cli --version "^2"
```

### Running in Development

```bash
npm run tauri dev
```

This starts Vite's HMR dev server on `http://localhost:1420` and opens the Tauri desktop window. The Rust backend and the Vue frontend hot-reload independently.

To preview the Vue UI in a browser without Rust (no agent/chart functionality):

```bash
npm run dev
# then open http://localhost:1420
```

### Building for Production

```bash
npm run tauri build
```

Produces a native installer in `src-tauri/target/release/bundle/`.

---

## Tutorial: Generate Your First Strategy

### Step 1 — Configure your LLM provider

Both goose (primary engine) and the Python fallback share the same set of
environment variables.  Set `LLM_PROVIDER` to one of the supported providers
and supply the matching API key:

```bash
# OpenAI (default)
export LLM_PROVIDER=openai
export LLM_API_KEY=sk-...           # or export OPENAI_API_KEY=sk-...

# Kimi (Moonshot AI)
export LLM_PROVIDER=kimi
export LLM_API_KEY=<moonshot-key>   # or export KIMI_API_KEY=<moonshot-key>

# GLM (Zhipu AI)
export LLM_PROVIDER=glm
export LLM_API_KEY=<zhipu-key>      # or export GLM_API_KEY=<zhipu-key>

# SiliconFlow
export LLM_PROVIDER=siliconflow
export LLM_API_KEY=<sf-key>         # or export SILICONFLOW_API_KEY=<sf-key>

# Azure OpenAI
export LLM_PROVIDER=azure
export AZURE_OPENAI_API_KEY=<key>
export AZURE_OPENAI_ENDPOINT=https://<resource>.openai.azure.com/
export AZURE_OPENAI_DEPLOYMENT=<deployment-name>
# export AZURE_OPENAI_API_VERSION=2024-02-01   # optional, shown default

# Optional overrides (any provider)
export LLM_MODEL=<model-name>       # override the provider's default model
export LLM_BASE_URL=<url>           # override the API base URL
```

If none of the above is set, the Python fallback uses its built-in
`template_bt.py` without calling an LLM.  Goose manages its own model when
it is configured separately via `goose configure`.

### Step 2 — Launch the app

```bash
npm run tauri dev
```

The app opens with the **策略研发室 (Strategy Lab)** tab active.

### Step 3 — Enter a trading idea

In the top-left text area, describe your strategy in plain language. Examples:

```
5日均线与20日均线金叉策略，标的为平安银行 000001
```
```
RSI(14) 超买超卖策略，RSI > 70 卖出，RSI < 30 买入，标的为贵州茅台 600519
```
```
布林带突破策略，价格突破上轨买入，跌破下轨卖出，标的为宁德时代 300750
```

### Step 4 — Click ▶ 开始研发

The terminal pane on the right fills with colour-coded agent logs:

| Colour | Prefix | Meaning |
|---|---|---|
| Cyan | `[AGENT_THINKING]` | Reasoning / planning |
| Yellow | `[AGENT_EXECUTING]` | Running a shell command |
| Green | `[AGENT_SUCCESS]` | Step completed |
| Red | `[AGENT_ERROR]` | Something failed |
| Magenta | `[AGENT_FIXING]` | Retrying with a fix |

### Step 5 — View backtest results

Once the agent prints `[AGENT_SUCCESS] Strategy executed successfully!`, the right panel automatically updates with:

- **Sharpe ratio** (green = ≥ 1, amber = 0–1, red = < 0)
- **Max drawdown %** (always red — lower is better)
- **Total return %** (green = positive)
- **Number of trades**
- **Equity curve** area chart (2022-01-01 → 2023-12-31)

### Step 6 — Monitor strategies

Switch to the **监控仪表盘 (Monitor Dashboard)** tab to see all saved strategies. Use the toggle switch on each card to enable or disable monitoring. Enabled strategies receive desktop notifications from the Rust market engine every 60 seconds.

---

## UI Walkthrough

```
┌─────────────────────────────────────────────────────────────────────┐
│ ⬡ AI QUANT TERMINAL  v0.1.0          ● 市场引擎    14:32:01        │
├──────────┬──────────────────────────────────────────────────────────┤
│          │  ⚗ 策略研发 — 输入你的量化灵感                           │
│ ⚗ 策略   │  ┌────────────────────────────────────────────────────┐  │
│  研发室  │  │ 5日均线与20日均线金叉策略，标的为平安银行 000001    │  │
│          │  └────────────────────────────────────────────────────┘  │
│ 📡 监控   │  [ ▶ 开始研发 ]                                          │
│  仪表盘  ├────────────────────────────┬───────────────────────────┤ │
│          │ ╔════════════════════════╗ │ 📈 回测结果               │ │
│          │ ║  AI QUANT TERMINAL     ║ │ ┌──────────┬────────────┐ │ │
│          │ ║  AGENT LOG             ║ │ │ 夏普比率 │ 最大回撤   │ │ │
│          │ ╚════════════════════════╝ │ │   1.23   │  -8.45%   │ │ │
│          │                           │ ├──────────┼────────────┤ │ │
│ ─────── │ [AGENT_THINKING] Starting  │ │ 总收益率 │ 交易次数   │ │ │
│ OPENAI  │ [AGENT_EXECUTING] python3  │ │  +24.7%  │    47      │ │ │
│ GPT-4o  │ [AGENT_SUCCESS] Sharpe:1.2 │ └──────────┴────────────┘ │ │
│ akshare │                            │                            │ │
│backtrader│                            │   ╱╲   ╱╲   ╱╲╱╲╱       │ │
│          │                            │  ╱  ╲ ╱  ╲ ╱           │ │ │
│          │                            │ ╱    ╲╱    ╲            │ │ │
├──────────┴────────────────────────────┴───────────────────────────┘ │
│ ✅ Agent 执行完成                               活跃策略: 2          │
└─────────────────────────────────────────────────────────────────────┘
```

**Layout zones:**

| Zone | Component | Description |
|---|---|---|
| Header | `App.vue` | App name, market engine status LED, last heartbeat time |
| Sidebar | `App.vue` | Tab navigation — Strategy Lab / Monitor Dashboard |
| Strategy input | `StrategyInput.vue` | Multi-line textarea + run button with loading animation |
| Agent terminal | `AgentTerminal.vue` | Full xterm.js emulator, scrollback 5 000 lines |
| Backtest chart | `BacktestChart.vue` | Metrics 2×2 grid + ECharts equity-curve area chart |
| Monitor panel | `MonitorPanel.vue` | Responsive 1–3 column card grid with toggle switches |
| Status bar | `App.vue` | Current operation text + active strategy count |

---

## Design Details

### Rust Backend

#### `agent_runner.rs`

The `run_agent` Tauri command is the bridge between UI and AI:

1. **`find_goose()`** probes multiple PATH locations (`/usr/local/bin/goose`, `~/.local/bin/goose`, …) so goose works even before the shell profile is re-sourced after install.
2. **`run_with_goose()`** spawns `goose run --recipe python_agent/quant_strategy.yaml --params idea=<idea> --no-session` and pipes stdout line-by-line to the frontend via the `agent-log` Tauri event.
3. **`run_with_python()`** is the automatic fallback — it spawns `python3 python_agent/agent_loop.py --idea <idea> --force-python` and streams its stdout the same way.
4. The frontend receives `agent-log` events regardless of which engine ran — the terminal pane is engine-agnostic.

#### `market_engine.rs`

Runs a background Tokio task at app start:

- **60-second `interval`** ticker reads `strategies.json` on every tick.
- Emits a `market-heartbeat` event to the frontend with `{ timestamp, active_strategies }`.
- For each enabled strategy whose `last_trigger` is `null`, calls `send_alert()` to fire a native OS desktop notification.

The loop uses an `Arc<AtomicBool>` flag so it can be stopped gracefully (useful for testing and future stop/restart UI controls).

#### `notifier.rs`

Thin wrapper around the `notify-rust` crate. Uses the `dialog-information` icon and falls back silently if the notification daemon is unavailable.

### AI Agent Pipeline

```
User idea (natural language)
        │
        ▼
  ┌─────────────────────────────────────────────────────┐
  │  quant_strategy.yaml (goose recipe)                  │
  │                                                      │
  │  STEP 1: Generate temp_strategy.py via developer ext │
  │  STEP 2: bash python3 temp_strategy.py               │
  │  STEP 3: Evaluate — valid JSON metrics?              │
  │     YES ──► STEP 5                                   │
  │     NO  ──► STEP 4                                   │
  │  STEP 4: Self-heal (read stderr, rewrite, retry ×5)  │
  │  STEP 5: Write report.json + append strategies.json  │
  └─────────────────────────────────────────────────────┘
        │
        ▼ (if goose unavailable)
  ┌─────────────────────────────────────────────────────┐
  │  agent_loop.py — Python fallback                     │
  │                                                      │
  │  • LangChain with configurable LLM provider          │
  │    (openai / kimi / glm / siliconflow / azure)       │
  │  • Conversation history for multi-turn self-healing  │
  │  • Same 5-retry loop + identical output format       │
  │  • Falls back to template_bt.py if no API key is set │
  └─────────────────────────────────────────────────────┘
```

**Strategy script output contract** — the generated Python file must print exactly one JSON object:

```json
{
  "sharpe_ratio": 1.23,
  "max_drawdown_pct": 8.45,
  "total_return_pct": 24.7,
  "num_trades": 47,
  "final_portfolio_value": 124700.0,
  "equity_curve": [
    {"date": "2022-01-04", "value": 100000.0},
    {"date": "2022-01-05", "value": 100230.0}
  ]
}
```

### Frontend Components

#### `StrategyInput.vue`

- Emits `agent-started` and `agent-done(success: boolean)` to `App.vue` to update the status bar.
- Disables the textarea and button while the agent runs (guards against double-submission).
- An animated `⟳` spinner with a three-dot ellipsis indicates progress.

#### `AgentTerminal.vue`

- Mounts a real `Terminal` instance from `@xterm/xterm` with a matrix-green-on-black theme.
- Uses `FitAddon` + `ResizeObserver` so the terminal fills its container at any window size.
- `colorize(line)` maps log prefixes to ANSI escape codes:
  - `[AGENT_THINKING]` → cyan
  - `[AGENT_EXECUTING]` → yellow
  - `[AGENT_SUCCESS]` → green
  - `[AGENT_ERROR]` → red
  - `[AGENT_FIXING]` → magenta

#### `BacktestChart.vue`

- Initialises an ECharts instance with a dark terminal palette (green area chart, dashed grey grid).
- Reads `report.json` via the Tauri FS plugin on mount and after every `[AGENT_SUCCESS]` log line.
- Falls back to 60-point sine-wave demo data when running in browser mode (no Tauri FS available).
- `metricClass()` applies green / amber / red colours to metrics depending on threshold values.

#### `MonitorPanel.vue`

- Polls `strategies.json` every **30 seconds** and also refreshes on every `[AGENT_SUCCESS]` event.
- Calls `invoke('toggle_strategy', { id, enabled })` to persist toggle state back through the Rust backend.
- Responsive grid: 1 column on mobile, 2 on medium, 3 on extra-large screens.

### Data Flow

```
python_agent/workspace/
  ├── temp_strategy.py    ← written by AI agent each run
  ├── report.json         ← latest backtest results (read by BacktestChart)
  └── strategies.json     ← append-only log of all strategies
                            (read by MonitorPanel + market_engine)
```

All persistence is plain JSON on disk — no database, no cloud, no telemetry.

---

## Configuration

| File | Key setting | Default |
|---|---|---|
| `src-tauri/tauri.conf.json` | Window size | 1400 × 900 px |
| `src-tauri/tauri.conf.json` | Min window size | 1024 × 600 px |
| `python_agent/quant_strategy.yaml` | `max_retries` | 5 |
| `python_agent/quant_strategy.yaml` | `workspace_dir` | `python_agent/workspace` |
| `python_agent/agent_loop.py` | `MAX_RETRIES` | 5 |
| `market_engine.rs` | Heartbeat interval | 60 s |
| `MonitorPanel.vue` | Poll interval | 30 s |
| `AgentTerminal.vue` | Terminal scrollback | 5 000 lines |

Backtest defaults (set inside generated strategy scripts):

| Parameter | Default |
|---|---|
| Start date | 2022-01-01 |
| End date | 2023-12-31 |
| Initial cash | ¥100,000 CNY |

---

## Environment Variables

Both the **goose** engine and the **Python fallback** read the same set of
variables.  `agent_runner.rs` automatically translates them to goose's native
variables (`GOOSE_PROVIDER`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `GOOSE_MODEL`)
when spawning the goose subprocess.

### Shared config

| Variable | Required | Description |
|---|---|---|
| `LLM_PROVIDER` | No (default: `openai`) | Provider: `openai`, `kimi`, `glm`, `siliconflow`, or `azure` |
| `LLM_API_KEY` | Yes* | API key for the chosen provider. Also accepted as a fallback when the provider-specific var is absent. |
| `LLM_MODEL` | No | Override the default model for the chosen provider |
| `LLM_BASE_URL` | No | Override the API base URL (useful for any OpenAI-compatible endpoint) |

*Not required when using the built-in template fallback or when the provider-specific key variable is set.

### Provider-specific API key variables

These are checked **before** `LLM_API_KEY` for their respective providers:

| Variable | Provider |
|---|---|
| `OPENAI_API_KEY` | `openai` |
| `KIMI_API_KEY` | `kimi` |
| `GLM_API_KEY` | `glm` |
| `SILICONFLOW_API_KEY` | `siliconflow` |

### Azure OpenAI

| Variable | Required | Description |
|---|---|---|
| `AZURE_OPENAI_API_KEY` | Yes | Azure OpenAI API key |
| `AZURE_OPENAI_ENDPOINT` | Yes | Endpoint URL, e.g. `https://<resource>.openai.azure.com/` |
| `AZURE_OPENAI_DEPLOYMENT` | Yes | Deployment / model name |
| `AZURE_OPENAI_API_VERSION` | No (default: `2024-02-01`) | Azure API version |

### Provider defaults

| Provider | Default model | API base URL |
|---|---|---|
| `openai` | `gpt-4o-mini` | OpenAI SDK default (`https://api.openai.com/v1`) |
| `kimi` | `moonshot-v1-8k` | `https://api.moonshot.cn/v1` |
| `glm` | `glm-4-flash` | `https://open.bigmodel.cn/api/paas/v4` |
| `siliconflow` | `Qwen/Qwen2.5-7B-Instruct` | `https://api.siliconflow.cn/v1` |
| `azure` | from `AZURE_OPENAI_DEPLOYMENT` | from `AZURE_OPENAI_ENDPOINT` |
