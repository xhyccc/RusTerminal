"""
Data fetching utilities for AI Quant Terminal.

This module is a thin wrapper around data_cache, which provides transparent
local caching and automatic multi-source fallback (akshare → yfinance → Sina).
Import fetch_stock_data from here for all data needs.
"""

from data_cache import fetch_stock_data  # noqa: F401  (re-exported public API)

__all__ = ["fetch_stock_data"]
