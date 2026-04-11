"""
Shared data cache for AI Quant Terminal.

Priority order for data sources (source="auto"):
  1. Local CSV cache  (python_agent/workspace/cache/<symbol>_<start>_<end>.csv)
  2. akshare          (East Money A-share data — default, most reliable)
  3. yfinance         (fallback; appends .SS or .SZ suffix automatically)
  4. Sina Finance     (via akshare's stock_zh_a_daily interface)

Set DATA_SOURCE env var to force a specific source: akshare | yfinance | sina
Set CACHE_STALE_DAYS env var to control staleness (default: 1 day).

Usage:
    from data_cache import fetch_stock_data
    df = fetch_stock_data("000001", "20220101", "20231231")
"""

from __future__ import annotations

import os
from datetime import datetime, timedelta
from pathlib import Path

import pandas as pd

CACHE_DIR = Path(__file__).parent / "workspace" / "cache"
CACHE_DIR.mkdir(parents=True, exist_ok=True)

# Cache treated as fresh when end_date is this many days before today.
CACHE_STALE_DAYS = int(os.environ.get("CACHE_STALE_DAYS", "1"))


# ---------------------------------------------------------------------------
# Cache helpers
# ---------------------------------------------------------------------------

def _cache_path(symbol: str, start_date: str, end_date: str) -> Path:
    return CACHE_DIR / f"{symbol}_{start_date}_{end_date}.csv"


def _is_cache_valid(path: Path, end_date: str) -> bool:
    """Return True when cache file exists and is not stale."""
    if not path.exists():
        return False
    try:
        end_dt = datetime.strptime(end_date, "%Y%m%d").date()
    except ValueError:
        return False
    today = datetime.today().date()
    # Historical range (end is well in the past) — always valid once written.
    if end_dt < today - timedelta(days=CACHE_STALE_DAYS):
        return True
    # Recent data — valid only if file was written today.
    mtime_date = datetime.fromtimestamp(path.stat().st_mtime).date()
    return mtime_date >= today - timedelta(days=CACHE_STALE_DAYS)


def _load_cache(path: Path) -> pd.DataFrame:
    return pd.read_csv(path, parse_dates=["date"])


def _save_cache(df: pd.DataFrame, path: Path) -> None:
    df.to_csv(path, index=False)


# ---------------------------------------------------------------------------
# Data source implementations
# ---------------------------------------------------------------------------

def _fetch_akshare(symbol: str, start_date: str, end_date: str) -> pd.DataFrame:
    import akshare as ak  # type: ignore
    df = ak.stock_zh_a_hist(
        symbol=symbol,
        period="daily",
        start_date=start_date,
        end_date=end_date,
        adjust="qfq",
    )
    df = df.rename(columns={
        "日期": "date", "开盘": "open", "最高": "high",
        "最低": "low", "收盘": "close", "成交量": "volume",
    })
    df["date"] = pd.to_datetime(df["date"])
    df = df.sort_values("date").reset_index(drop=True)
    return df[["date", "open", "high", "low", "close", "volume"]]


def _yf_ticker(symbol: str) -> str:
    """Map a bare A-share code to its Yahoo Finance ticker.

    Shanghai Stock Exchange codes all start with 6 (main board 6xxxxx,
    STAR Market 688xxx).  All other numeric codes belong to Shenzhen
    (main board 0xxxxx, Growth Enterprise 3xxxxx, B-shares 2xxxxx).
    """
    return symbol + (".SS" if symbol.startswith("6") else ".SZ")


def _fetch_yfinance(symbol: str, start_date: str, end_date: str) -> pd.DataFrame:
    import yfinance as yf  # type: ignore
    start = datetime.strptime(start_date, "%Y%m%d").strftime("%Y-%m-%d")
    end = datetime.strptime(end_date, "%Y%m%d").strftime("%Y-%m-%d")
    ticker = yf.Ticker(_yf_ticker(symbol))
    df = ticker.history(start=start, end=end, auto_adjust=True)
    if df.empty:
        raise ValueError(f"yfinance returned no data for {symbol}")
    df = df.reset_index().rename(columns={
        "Date": "date", "Open": "open", "High": "high",
        "Low": "low", "Close": "close", "Volume": "volume",
    })
    # Strip timezone so downstream code is tz-naïve.
    df["date"] = pd.to_datetime(df["date"]).dt.tz_localize(None)
    df = df.sort_values("date").reset_index(drop=True)
    return df[["date", "open", "high", "low", "close", "volume"]]


def _fetch_sina(symbol: str, start_date: str, end_date: str) -> pd.DataFrame:
    """Fetch via akshare's Sina Finance daily interface."""
    import akshare as ak  # type: ignore
    prefix = "sz" if not symbol.startswith("6") else "sh"
    df = ak.stock_zh_a_daily(symbol=f"{prefix}{symbol}", adjust="qfq")
    df = df.reset_index()
    # Normalise column names — akshare returns English columns here.
    df = df.rename(columns={
        "date": "date", "open": "open", "high": "high",
        "low": "low", "close": "close", "volume": "volume",
    })
    df["date"] = pd.to_datetime(df["date"])
    start_dt = pd.to_datetime(start_date, format="%Y%m%d")
    end_dt = pd.to_datetime(end_date, format="%Y%m%d")
    df = df[(df["date"] >= start_dt) & (df["date"] <= end_dt)]
    df = df.sort_values("date").reset_index(drop=True)
    if df.empty:
        raise ValueError(f"Sina Finance returned no data for {symbol}")
    return df[["date", "open", "high", "low", "close", "volume"]]


_SOURCE_FNS = {
    "akshare": _fetch_akshare,
    "yfinance": _fetch_yfinance,
    "sina": _fetch_sina,
}

# Default order when source="auto".  akshare is tried first because it is
# already a dependency and provides the most reliable A-share data.
_AUTO_ORDER = ["akshare", "yfinance", "sina"]


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def fetch_stock_data(
    symbol: str,
    start_date: str,
    end_date: str,
    source: str = "auto",
) -> pd.DataFrame:
    """
    Fetch stock OHLCV data with transparent local caching.

    Args:
        symbol:     A-share stock code, e.g. "000001".
        start_date: Start date string "YYYYMMDD".
        end_date:   End date string "YYYYMMDD".
        source:     "auto" (cache → akshare → yfinance → sina) or
                    an explicit source name: "akshare" | "yfinance" | "sina".

    Returns:
        DataFrame with columns: date (datetime64), open, high, low, close, volume.
    """
    # Normalise dates in case callers pass "YYYY-MM-DD".
    start_date = start_date.replace("-", "")
    end_date = end_date.replace("-", "")

    cache_path = _cache_path(symbol, start_date, end_date)

    # 1. Serve from cache when available and fresh.
    if _is_cache_valid(cache_path, end_date):
        print(f"[DATA_CACHE] Cache hit: {cache_path.name}", flush=True)
        return _load_cache(cache_path)

    # 2. Fetch from network source(s).
    env_source = os.environ.get("DATA_SOURCE", "").lower()
    if env_source and env_source in _SOURCE_FNS:
        source = env_source

    sources = _AUTO_ORDER if source == "auto" else [source]
    last_exc: Exception = RuntimeError("No data sources configured")

    for src in sources:
        fn = _SOURCE_FNS.get(src)
        if fn is None:
            continue
        try:
            print(f"[DATA_CACHE] Fetching {symbol} {start_date}→{end_date} via {src}", flush=True)
            df = fn(symbol, start_date, end_date)
            if df.empty:
                raise ValueError("Empty DataFrame returned")
            _save_cache(df, cache_path)
            print(f"[DATA_CACHE] Cached {len(df)} rows → {cache_path.name}", flush=True)
            return df
        except Exception as exc:
            print(f"[DATA_CACHE] {src} failed: {exc}", flush=True)
            last_exc = exc

    raise RuntimeError(
        f"All data sources failed for {symbol} ({start_date}→{end_date}): {last_exc}"
    ) from last_exc
