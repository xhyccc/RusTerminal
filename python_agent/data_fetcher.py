"""Data fetching utilities using akshare."""

import akshare as ak
import pandas as pd


def fetch_stock_data(symbol: str, start_date: str, end_date: str) -> pd.DataFrame:
    """
    Fetch A-share stock historical data via akshare.

    Args:
        symbol: Stock code, e.g. "000001" for Ping An Bank
        start_date: Start date string "YYYYMMDD"
        end_date: End date string "YYYYMMDD"

    Returns:
        DataFrame with columns: date, open, high, low, close, volume
    """
    df = ak.stock_zh_a_hist(
        symbol=symbol,
        period="daily",
        start_date=start_date,
        end_date=end_date,
        adjust="qfq",
    )

    df = df.rename(
        columns={
            "日期": "date",
            "开盘": "open",
            "最高": "high",
            "最低": "low",
            "收盘": "close",
            "成交量": "volume",
            "成交额": "amount",
            "振幅": "amplitude",
            "涨跌幅": "pct_change",
            "涨跌额": "change",
            "换手率": "turnover",
        }
    )

    df["date"] = pd.to_datetime(df["date"])
    df = df.sort_values("date").reset_index(drop=True)
    return df[["date", "open", "high", "low", "close", "volume"]]
