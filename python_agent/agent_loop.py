"""
Goose-powered CLI agent for quantitative strategy generation.

Primary engine: goose (https://github.com/aaif-goose/goose) — a Rust-built,
open-source AI agent that uses the quant_strategy.yaml recipe.

Fallback engine: built-in Python agent using LangChain / OpenAI when goose
is not installed or the user explicitly opts out.

Install goose:
    curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash

Usage:
    python agent_loop.py --idea "5日均线与20日均线金叉策略，标的为平安银行"
    python agent_loop.py --idea "..." --force-python   # skip goose
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import uuid
from datetime import datetime
from pathlib import Path
from typing import Any

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

AGENT_DIR = Path(__file__).parent
PROJECT_ROOT = AGENT_DIR.parent          # repo root — where config.yaml lives
RECIPE_FILE = AGENT_DIR / "quant_strategy.yaml"
WORKSPACE = AGENT_DIR / "workspace"
WORKSPACE.mkdir(exist_ok=True)
TEMP_STRATEGY = WORKSPACE / "temp_strategy.py"
REPORT_FILE = WORKSPACE / "report.json"
STRATEGIES_FILE = WORKSPACE / "strategies.json"

MAX_RETRIES = 5  # may be overridden by _load_config()

# ---------------------------------------------------------------------------
# YAML configuration loader
# ---------------------------------------------------------------------------
# config.yaml lives at the project root.  Values in that file serve as
# defaults; environment variables always take precedence.


def _load_config() -> None:
    """
    Load config.yaml from the project root and apply its values as env-var
    defaults.  Environment variables already set in the shell are never
    overwritten.  This function also updates the module-level MAX_RETRIES
    constant when agent.max_retries is present in the config.
    """
    global MAX_RETRIES

    config_path = PROJECT_ROOT / "config.yaml"
    if not config_path.exists():
        return  # config.yaml is optional

    try:
        import yaml  # PyYAML — installed by build.sh
    except ImportError:
        print(
            "[AGENT_THINKING] pyyaml not installed — skipping config.yaml. "
            "Run: pip install pyyaml",
            flush=True,
        )
        return

    try:
        with config_path.open() as fh:
            cfg: dict = yaml.safe_load(fh) or {}
    except Exception as exc:
        print(f"[AGENT_THINKING] Could not parse config.yaml: {exc}", flush=True)
        return

    # ── LLM section ──────────────────────────────────────────────────────────
    llm = cfg.get("llm") or {}

    def _set_default(env_var: str, value: Any) -> None:
        """Set env_var only when it is not already present in the environment
        and value is non-empty/non-None."""
        if value and not os.environ.get(env_var):
            os.environ[env_var] = str(value)

    _set_default("LLM_PROVIDER", llm.get("provider"))
    _set_default("LLM_API_KEY",  llm.get("api_key"))
    _set_default("LLM_MODEL",    llm.get("model"))
    _set_default("LLM_BASE_URL", llm.get("base_url"))

    azure = llm.get("azure") or {}
    _set_default("AZURE_OPENAI_ENDPOINT",    azure.get("endpoint"))
    _set_default("AZURE_OPENAI_DEPLOYMENT",  azure.get("deployment"))
    _set_default("AZURE_OPENAI_API_VERSION", azure.get("api_version"))

    # ── Agent section ─────────────────────────────────────────────────────────
    agent_cfg = cfg.get("agent") or {}
    if "max_retries" in agent_cfg:
        try:
            MAX_RETRIES = int(agent_cfg["max_retries"])
        except (TypeError, ValueError) as exc:
            print(
                f"[AGENT_THINKING] config.yaml: invalid agent.max_retries value "
                f"({agent_cfg['max_retries']!r}) — using default {MAX_RETRIES}. Error: {exc}",
                flush=True,
            )

    backtest = agent_cfg.get("backtest") or {}
    _set_default("BACKTEST_START_DATE",  backtest.get("start_date"))
    _set_default("BACKTEST_END_DATE",    backtest.get("end_date"))
    _set_default("BACKTEST_INITIAL_CASH", backtest.get("initial_cash"))


# Load the config at import time so all subsequent code sees the defaults.
_load_config()

# ---------------------------------------------------------------------------
# Goose engine
# ---------------------------------------------------------------------------


def _find_goose() -> str | None:
    """Return the path to the goose binary if it is installed."""
    return shutil.which("goose")


def _run_with_goose(idea: str) -> int:
    """
    Invoke goose with the quant_strategy.yaml recipe and stream its output
    directly to our own stdout so Tauri can capture it line-by-line.

    Returns the process exit code.
    """
    goose = _find_goose()
    if goose is None:
        print(
            "[AGENT_THINKING] goose binary not found in PATH. "
            "Install it with: "
            "curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash",
            flush=True,
        )
        return 1

    if not RECIPE_FILE.exists():
        print(f"[AGENT_ERROR] Recipe file not found: {RECIPE_FILE}", flush=True)
        return 1

    print(f"[AGENT_THINKING] Starting goose agent — idea: {idea}", flush=True)
    print(f"[AGENT_THINKING] Recipe: {RECIPE_FILE}", flush=True)

    cmd = [
        goose, "run",
        "--recipe", str(RECIPE_FILE),
        "--params", f"idea={idea}",
        "--no-session",
    ]

    proc = subprocess.run(
        cmd,
        # Inherit stdout/stderr so output passes through to the caller
        # (Tauri reads our stdout line-by-line).
        stdout=sys.stdout,
        stderr=sys.stderr,
    )
    return proc.returncode


# ---------------------------------------------------------------------------
# Python fallback engine (LangChain / OpenAI)
# ---------------------------------------------------------------------------

SYSTEM_PROMPT = """\
You are an expert quantitative analyst who writes Backtrader strategies for Chinese A-share markets.

Rules:
1. Fetch market data using the shared cache module (already in the same directory):
       import sys
       import os
       sys.path.insert(0, os.path.dirname(__file__))
       from data_cache import fetch_stock_data
       df = fetch_stock_data(symbol, start_date_yyyymmdd, end_date_yyyymmdd)
   This tries local CSV cache first, then falls back to akshare, yfinance, and Sina Finance.
   Do NOT call akshare, yfinance, or pandas_datareader directly.
2. The script must be fully self-contained and runnable with `python3 <file>`.
3. At the end of the script, print ONE JSON object to stdout with these keys:
   - sharpe_ratio   (float or null)
   - max_drawdown_pct  (float, percentage e.g. 12.5 means 12.5%)
   - total_return_pct  (float, percentage)
   - num_trades     (int)
   - final_portfolio_value (float)
   - equity_curve   (list of {"date": "YYYY-MM-DD", "value": float})
4. Do NOT print anything else to stdout.
5. Use `print(json.dumps(metrics))` at the very end.
6. Default backtest period: 20220101 to 20231231, initial cash 100000 CNY.
"""


# ---------------------------------------------------------------------------
# LLM provider configuration
# ---------------------------------------------------------------------------
# Set LLM_PROVIDER to one of: openai, kimi, glm, siliconflow, azure
# Set LLM_API_KEY  for the chosen provider (or use the provider-specific key).
# Set LLM_MODEL    to override the default model for the provider.
# Set LLM_BASE_URL to override the API base URL (for any OpenAI-compatible provider).
#
# Provider-specific API key env vars (checked before LLM_API_KEY):
#   openai      → OPENAI_API_KEY
#   kimi        → KIMI_API_KEY
#   glm         → GLM_API_KEY
#   siliconflow → SILICONFLOW_API_KEY
#   azure       → AZURE_OPENAI_API_KEY  (also needs AZURE_OPENAI_ENDPOINT,
#                                         AZURE_OPENAI_DEPLOYMENT, optionally
#                                         AZURE_OPENAI_API_VERSION)
# ---------------------------------------------------------------------------

_PROVIDER_DEFAULTS: dict[str, dict] = {
    "openai":      {"base_url": None,                                    "model": "gpt-4o-mini"},
    "kimi":        {"base_url": "https://api.moonshot.cn/v1",            "model": "moonshot-v1-8k"},
    "glm":         {"base_url": "https://open.bigmodel.cn/api/paas/v4",  "model": "glm-4-flash"},
    "siliconflow": {"base_url": "https://api.siliconflow.cn/v1",         "model": "Qwen/Qwen2.5-7B-Instruct"},
}

_PROVIDER_KEY_VARS: dict[str, list[str]] = {
    "openai":      ["OPENAI_API_KEY", "LLM_API_KEY"],
    "kimi":        ["KIMI_API_KEY",        "LLM_API_KEY", "OPENAI_API_KEY"],
    "glm":         ["GLM_API_KEY",         "LLM_API_KEY", "OPENAI_API_KEY"],
    "siliconflow": ["SILICONFLOW_API_KEY", "LLM_API_KEY", "OPENAI_API_KEY"],
}


def _resolve_api_key(provider: str) -> str:
    for var in _PROVIDER_KEY_VARS.get(provider, ["LLM_API_KEY", "OPENAI_API_KEY"]):
        val = os.environ.get(var, "")
        if val:
            return val
    return ""


def _build_azure_llm():
    """Build an AzureChatOpenAI client from AZURE_OPENAI_* env vars."""
    api_key  = os.environ.get("AZURE_OPENAI_API_KEY", "")
    endpoint = os.environ.get("AZURE_OPENAI_ENDPOINT", "")
    deploy   = os.environ.get("AZURE_OPENAI_DEPLOYMENT", "")
    version  = os.environ.get("AZURE_OPENAI_API_VERSION", "2024-02-01")

    if not (api_key and endpoint and deploy):
        return None

    try:
        from langchain_openai import AzureChatOpenAI
        return AzureChatOpenAI(
            azure_deployment=deploy,
            azure_endpoint=endpoint,
            api_key=api_key,
            api_version=version,
            temperature=0.2,
        )
    except ImportError:
        try:
            from langchain.chat_models import AzureChatOpenAI  # type: ignore
            return AzureChatOpenAI(
                deployment_name=deploy,
                openai_api_base=endpoint,
                openai_api_key=api_key,
                openai_api_version=version,
                temperature=0.2,
            )
        except ImportError:
            return None


def _build_llm():
    """Build a LangChain chat model based on LLM_PROVIDER (default: openai)."""
    provider = os.environ.get("LLM_PROVIDER", "openai").lower()

    if provider == "azure":
        return _build_azure_llm()

    api_key = _resolve_api_key(provider)
    if not api_key:
        return None

    defaults = _PROVIDER_DEFAULTS.get(provider, _PROVIDER_DEFAULTS["openai"])
    # LLM_BASE_URL overrides the provider's default base URL
    base_url = os.environ.get("LLM_BASE_URL") or defaults["base_url"]
    model    = os.environ.get("LLM_MODEL")    or defaults["model"]

    try:
        from langchain_openai import ChatOpenAI
        kwargs: dict[str, Any] = {"model": model, "temperature": 0.2, "api_key": api_key}
        if base_url:
            kwargs["base_url"] = base_url
        return ChatOpenAI(**kwargs)
    except ImportError:
        try:
            from langchain.chat_models import ChatOpenAI  # type: ignore
            kwargs = {"model_name": model, "temperature": 0.2, "openai_api_key": api_key}
            if base_url:
                kwargs["openai_api_base"] = base_url
            return ChatOpenAI(**kwargs)
        except ImportError:
            return None


def _ask_llm(llm, messages: list[dict]) -> str:
    from langchain.schema import HumanMessage, SystemMessage, AIMessage  # type: ignore

    lc_messages = []
    for m in messages:
        role, content = m["role"], m["content"]
        if role == "system":
            lc_messages.append(SystemMessage(content=content))
        elif role == "user":
            lc_messages.append(HumanMessage(content=content))
        elif role == "assistant":
            lc_messages.append(AIMessage(content=content))

    return llm.invoke(lc_messages).content


def _extract_code(raw: str) -> str:
    """Strip markdown fences if present."""
    if "```" not in raw:
        return raw.strip()
    lines = raw.strip().splitlines()
    in_block = False
    result: list[str] = []
    for line in lines:
        if line.startswith("```"):
            in_block = not in_block
            continue
        if in_block:
            result.append(line)
    return "\n".join(result).strip()


def _run_script(script_path: Path) -> tuple[bool, str, str]:
    python_cmd = "python" if sys.platform == "win32" else "python3"
    r = subprocess.run(
        [python_cmd, str(script_path)],
        capture_output=True,
        text=True,
        timeout=120,
    )
    return r.returncode == 0, r.stdout, r.stderr


def _parse_metrics(stdout: str) -> dict | None:
    for line in reversed(stdout.strip().splitlines()):
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return None


def _save_report(metrics: dict, idea: str) -> None:
    report = {"idea": idea, "timestamp": datetime.now().isoformat(), "metrics": metrics}
    REPORT_FILE.write_text(json.dumps(report, ensure_ascii=False, indent=2))

    strategies: list[dict] = []
    if STRATEGIES_FILE.exists():
        try:
            strategies = json.loads(STRATEGIES_FILE.read_text())
        except Exception:
            strategies = []

    strategies.append(
        {
            "id": str(uuid.uuid4()),
            "idea": idea,
            "timestamp": datetime.now().isoformat(),
            "metrics": metrics,
            "enabled": True,
            "last_trigger": None,
        }
    )
    STRATEGIES_FILE.write_text(json.dumps(strategies, ensure_ascii=False, indent=2))


def _python_fallback(idea: str) -> None:
    """Legacy self-healing agent loop using LangChain/OpenAI."""
    print(f"[AGENT_THINKING] Python fallback agent — idea: {idea}", flush=True)

    llm = _build_llm()
    if llm is None:
        provider = os.environ.get("LLM_PROVIDER", "openai")
        print(
            f"[AGENT_THINKING] No API key found for provider '{provider}' "
            "— using built-in template",
            flush=True,
        )

    template = (AGENT_DIR / "template_bt.py").read_text()

    conversation: list[dict] = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {
            "role": "user",
            "content": (
                f"请根据以下量化灵感，编写一个完整的 Backtrader 策略脚本：\n\n{idea}\n\n"
                "只输出 Python 代码，不需要任何解释。"
            ),
        },
    ]

    code = ""
    last_error = ""

    for attempt in range(MAX_RETRIES + 1):
        if attempt == 0:
            print("[AGENT_THINKING] Generating strategy code …", flush=True)
            if llm:
                try:
                    raw = _ask_llm(llm, conversation)
                    code = _extract_code(raw)
                    conversation.append({"role": "assistant", "content": raw})
                except Exception as exc:
                    print(f"[AGENT_ERROR] LLM call failed: {exc} — using template", flush=True)
                    code = template
            else:
                code = template
        else:
            print(f"[AGENT_FIXING] Attempt {attempt}/{MAX_RETRIES} — fixing …", flush=True)
            if llm:
                conversation.append(
                    {
                        "role": "user",
                        "content": (
                            f"运行上述代码时出现了以下错误，请修复代码（仅输出修复后的完整 Python 代码）：\n\n{last_error}"
                        ),
                    }
                )
                try:
                    raw = _ask_llm(llm, conversation)
                    code = _extract_code(raw)
                    conversation.append({"role": "assistant", "content": raw})
                except Exception as exc:
                    print(f"[AGENT_ERROR] LLM fix failed: {exc}", flush=True)
                    break
            else:
                print("[AGENT_ERROR] Cannot fix without LLM — giving up", flush=True)
                break

        TEMP_STRATEGY.write_text(code)
        print(f"[AGENT_EXECUTING] python3 {TEMP_STRATEGY} (attempt {attempt + 1})", flush=True)

        try:
            success, stdout, stderr = _run_script(TEMP_STRATEGY)
        except subprocess.TimeoutExpired:
            last_error = "Script timed out after 120 seconds."
            print(f"[AGENT_ERROR] {last_error}", flush=True)
            continue

        if not success:
            last_error = stderr[-3000:]
            print(f"[AGENT_ERROR] Script error:\n{stderr[-500:]}", flush=True)
            continue

        metrics = _parse_metrics(stdout)
        if metrics is None:
            last_error = f"No valid JSON in stdout.\nstdout: {stdout[-1000:]}"
            print(f"[AGENT_ERROR] {last_error}", flush=True)
            continue

        print("[AGENT_SUCCESS] Strategy executed successfully!", flush=True)
        print(
            f"[AGENT_SUCCESS] Sharpe: {metrics.get('sharpe_ratio')} | "
            f"Max DD: {metrics.get('max_drawdown_pct')}% | "
            f"Total Return: {metrics.get('total_return_pct')}%",
            flush=True,
        )
        _save_report(metrics, idea)
        print(f"[AGENT_SUCCESS] Report saved to {REPORT_FILE}", flush=True)
        return

    print(f"[AGENT_ERROR] All {MAX_RETRIES} retries exhausted.", flush=True)
    sys.exit(1)


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="AI Quant Agent — goose-powered (https://github.com/aaif-goose/goose)"
    )
    parser.add_argument("--idea", required=True, help="Natural-language strategy description")
    parser.add_argument(
        "--force-python",
        action="store_true",
        help="Skip goose and use the Python fallback agent directly",
    )
    args = parser.parse_args()

    if args.force_python:
        _python_fallback(args.idea)
    else:
        rc = _run_with_goose(args.idea)
        if rc != 0:
            print(
                "[AGENT_THINKING] goose run failed or not available — falling back to Python agent",
                flush=True,
            )
            _python_fallback(args.idea)
