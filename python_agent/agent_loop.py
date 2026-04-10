"""
Self-healing CLI agent that generates, runs, and fixes Backtrader strategies
using an LLM (OpenAI via langchain).

Usage:
    python agent_loop.py --idea "5日均线与20日均线金叉策略，标的为平安银行"
"""

import argparse
import json
import os
import subprocess
import sys
import uuid
from datetime import datetime
from pathlib import Path

# ---------------------------------------------------------------------------
# LangChain / OpenAI wiring (gracefully degrade when key is absent)
# ---------------------------------------------------------------------------

WORKSPACE = Path(__file__).parent / "workspace"
WORKSPACE.mkdir(exist_ok=True)
TEMP_STRATEGY = WORKSPACE / "temp_strategy.py"
REPORT_FILE = WORKSPACE / "report.json"
STRATEGIES_FILE = WORKSPACE / "strategies.json"

MAX_RETRIES = 5

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
    from langchain.schema import HumanMessage, SystemMessage, AIMessage

    lc_messages = []
    for m in messages:
        role = m["role"]
        content = m["content"]
        if role == "system":
            lc_messages.append(SystemMessage(content=content))
        elif role == "user":
            lc_messages.append(HumanMessage(content=content))
        elif role == "assistant":
            lc_messages.append(AIMessage(content=content))

    response = llm.invoke(lc_messages)
    return response.content


def _extract_code(raw: str) -> str:
    """Strip markdown fences if present."""
    lines = raw.strip().splitlines()
    in_block = False
    result = []
    for line in lines:
        if line.startswith("```"):
            in_block = not in_block
            continue
        if in_block or not any(fence.startswith("```") for fence in ["```python", "```"]):
            result.append(line)
    # If no fences were found, return entire text
    if "```" not in raw:
        return raw.strip()
    return "\n".join(result).strip()


def _run_script(script_path: Path) -> tuple[bool, str, str]:
    """Run a Python script. Returns (success, stdout, stderr)."""
    python_cmd = "python" if sys.platform == "win32" else "python3"
    result = subprocess.run(
        [python_cmd, str(script_path)],
        capture_output=True,
        text=True,
        timeout=120,
    )
    return result.returncode == 0, result.stdout, result.stderr


def _parse_metrics(stdout: str) -> dict | None:
    """Try to parse the last JSON object from stdout."""
    for line in reversed(stdout.strip().splitlines()):
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return None


def _save_report(metrics: dict, idea: str):
    report = {"idea": idea, "timestamp": datetime.now().isoformat(), "metrics": metrics}
    REPORT_FILE.write_text(json.dumps(report, ensure_ascii=False, indent=2))

    # Append / update strategies.json
    strategies = []
    if STRATEGIES_FILE.exists():
        try:
            strategies = json.loads(STRATEGIES_FILE.read_text())
        except Exception:
            strategies = []

    entry = {
        "id": str(uuid.uuid4()),
        "idea": idea,
        "timestamp": datetime.now().isoformat(),
        "metrics": metrics,
        "enabled": True,
        "last_trigger": None,
    }
    strategies.append(entry)
    STRATEGIES_FILE.write_text(json.dumps(strategies, ensure_ascii=False, indent=2))


def _fallback_strategy(idea: str) -> str:
    """Return a simple template strategy when LLM is unavailable."""
    template = Path(__file__).parent / "template_bt.py"
    return template.read_text()


# ---------------------------------------------------------------------------
# Main agent loop
# ---------------------------------------------------------------------------

def run_agent(idea: str):
    print(f"[AGENT_THINKING] Initializing agent for idea: {idea}")

    llm = _build_llm()
    if llm is None:
        print("[AGENT_THINKING] No OPENAI_API_KEY found – using built-in template strategy")

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

    code: str = ""
    last_error: str = ""

    for attempt in range(MAX_RETRIES + 1):
        # ---------- generate / fix ----------
        if attempt == 0:
            print("[AGENT_THINKING] Generating strategy code …")
            if llm:
                try:
                    raw = _ask_llm(llm, conversation)
                    code = _extract_code(raw)
                    conversation.append({"role": "assistant", "content": raw})
                except Exception as exc:
                    print(f"[AGENT_ERROR] LLM call failed: {exc}. Falling back to template.")
                    code = _fallback_strategy(idea)
            else:
                code = _fallback_strategy(idea)
        else:
            print(f"[AGENT_FIXING] Attempt {attempt}/{MAX_RETRIES} – asking LLM to fix error …")
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
                    print(f"[AGENT_ERROR] LLM fix attempt failed: {exc}")
                    break
            else:
                print("[AGENT_ERROR] Cannot fix without LLM – exiting retry loop")
                break

        # ---------- persist & run ----------
        TEMP_STRATEGY.write_text(code)
        print(f"[AGENT_EXECUTING] Running strategy script (attempt {attempt + 1}) …")

        try:
            success, stdout, stderr = _run_script(TEMP_STRATEGY)
        except subprocess.TimeoutExpired:
            last_error = "Script timed out after 120 seconds."
            print(f"[AGENT_ERROR] {last_error}")
            continue

        if not success:
            last_error = stderr[-3000:]  # keep last 3K chars to avoid huge prompts
            print(f"[AGENT_ERROR] Script exited with error:\n{stderr[-500:]}")
            continue

        # ---------- parse metrics ----------
        metrics = _parse_metrics(stdout)
        if metrics is None:
            last_error = f"Script ran but produced no valid JSON.\nstdout: {stdout[-1000:]}"
            print(f"[AGENT_ERROR] {last_error}")
            continue

        # ---------- success ----------
        print(f"[AGENT_SUCCESS] Strategy executed successfully!")
        print(f"[AGENT_SUCCESS] Sharpe: {metrics.get('sharpe_ratio')} | "
              f"Max DD: {metrics.get('max_drawdown_pct')}% | "
              f"Total Return: {metrics.get('total_return_pct')}%")
        _save_report(metrics, idea)
        print(f"[AGENT_SUCCESS] Report saved to {REPORT_FILE}")
        return

    print(f"[AGENT_ERROR] All {MAX_RETRIES} retries exhausted. Strategy could not be fixed.")
    sys.exit(1)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="AI Quant Agent")
    parser.add_argument("--idea", required=True, help="Strategy idea description")
    args = parser.parse_args()
    run_agent(args.idea)
