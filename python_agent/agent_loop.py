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

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

AGENT_DIR = Path(__file__).parent
RECIPE_FILE = AGENT_DIR / "quant_strategy.yaml"
WORKSPACE = AGENT_DIR / "workspace"
WORKSPACE.mkdir(exist_ok=True)
TEMP_STRATEGY = WORKSPACE / "temp_strategy.py"
REPORT_FILE = WORKSPACE / "report.json"
STRATEGIES_FILE = WORKSPACE / "strategies.json"

MAX_RETRIES = 5

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
1. Use akshare (`ak.stock_zh_a_hist`) to fetch data.
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


def _build_llm():
    api_key = os.environ.get("OPENAI_API_KEY", "")
    if not api_key:
        return None
    try:
        from langchain_openai import ChatOpenAI
        return ChatOpenAI(model="gpt-4o-mini", temperature=0.2, api_key=api_key)
    except ImportError:
        try:
            from langchain.chat_models import ChatOpenAI  # type: ignore
            return ChatOpenAI(model_name="gpt-4o-mini", temperature=0.2, openai_api_key=api_key)
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
        print("[AGENT_THINKING] No OPENAI_API_KEY — using built-in template", flush=True)

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
