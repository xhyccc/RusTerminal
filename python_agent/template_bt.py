"""
Base Backtrader strategy template with SMA crossover and metrics extraction.
This file is used as a template by agent_loop.py and may be overwritten by the LLM.
"""

import sys
import json
import math
import akshare as ak
import pandas as pd
import backtrader as bt


# ---------------------------------------------------------------------------
# Data feed
# ---------------------------------------------------------------------------

class AkshareData(bt.feeds.PandasData):
    params = (
        ("datetime", None),
        ("open", "open"),
        ("high", "high"),
        ("low", "low"),
        ("close", "close"),
        ("volume", "volume"),
        ("openinterest", -1),
    )


def fetch_data(symbol: str = "000001", start_date: str = "20220101", end_date: str = "20231231"):
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
        }
    )
    df["date"] = pd.to_datetime(df["date"])
    df = df.set_index("date").sort_index()
    return df[["open", "high", "low", "close", "volume"]]


# ---------------------------------------------------------------------------
# Strategy
# ---------------------------------------------------------------------------

class SmaCrossStrategy(bt.Strategy):
    params = (
        ("fast_period", 10),
        ("slow_period", 30),
    )

    def __init__(self):
        self.fast_ma = bt.indicators.SMA(self.data.close, period=self.p.fast_period)
        self.slow_ma = bt.indicators.SMA(self.data.close, period=self.p.slow_period)
        self.crossover = bt.indicators.CrossOver(self.fast_ma, self.slow_ma)
        self.trade_log: list[dict] = []

    def next(self):
        if self.crossover > 0 and not self.position:
            self.buy()
        elif self.crossover < 0 and self.position:
            self.sell()

    def notify_trade(self, trade):
        if trade.isclosed:
            self.trade_log.append(
                {
                    "pnl": trade.pnl,
                    "pnlcomm": trade.pnlcomm,
                }
            )


# ---------------------------------------------------------------------------
# Metrics analyser
# ---------------------------------------------------------------------------

class MetricsAnalyzer(bt.Analyzer):
    def stop(self):
        portfolio_stats = self.strategy.analyzers.getbyname("time_return")
        returns = list(portfolio_stats.get_analysis().values()) if portfolio_stats else []

        sharpe_an = self.strategy.analyzers.getbyname("sharpe")
        sharpe = sharpe_an.get_analysis().get("sharperatio", None) if sharpe_an else None

        dd_an = self.strategy.analyzers.getbyname("drawdown")
        dd = dd_an.get_analysis() if dd_an else {}

        total_ret_an = self.strategy.analyzers.getbyname("returns")
        total_ret = total_ret_an.get_analysis().get("rtot", 0.0) if total_ret_an else 0.0

        self.rets = {
            "sharpe_ratio": round(sharpe, 4) if sharpe is not None else None,
            "max_drawdown_pct": round(dd.get("max", {}).get("drawdown", 0.0), 4),
            "total_return_pct": round(total_ret * 100, 4),
            "num_trades": len(self.strategy.trade_log),
        }


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

def run_backtest(symbol: str = "000001", start_date: str = "20220101", end_date: str = "20231231"):
    df = fetch_data(symbol, start_date, end_date)

    cerebro = bt.Cerebro()
    cerebro.addstrategy(SmaCrossStrategy, fast_period=10, slow_period=30)

    data_feed = AkshareData(dataname=df)
    cerebro.adddata(data_feed)

    cerebro.broker.setcash(100_000)
    cerebro.broker.setcommission(commission=0.001)
    cerebro.addsizer(bt.sizers.PercentSizer, percents=95)

    cerebro.addanalyzer(bt.analyzers.SharpeRatio, _name="sharpe", riskfreerate=0.03, annualize=True)
    cerebro.addanalyzer(bt.analyzers.DrawDown, _name="drawdown")
    cerebro.addanalyzer(bt.analyzers.Returns, _name="returns")
    cerebro.addanalyzer(bt.analyzers.TimeReturn, _name="time_return")

    results = cerebro.run()
    strat = results[0]

    sharpe = strat.analyzers.sharpe.get_analysis().get("sharperatio")
    dd = strat.analyzers.drawdown.get_analysis()
    ret = strat.analyzers.returns.get_analysis()

    metrics = {
        "sharpe_ratio": round(sharpe, 4) if sharpe is not None else None,
        "max_drawdown_pct": round(dd.get("max", {}).get("drawdown", 0.0), 4),
        "total_return_pct": round(ret.get("rtot", 0.0) * 100, 4),
        "num_trades": len(strat.trade_log),
        "final_portfolio_value": round(cerebro.broker.getvalue(), 2),
    }
    return metrics


if __name__ == "__main__":
    metrics = run_backtest()
    print(json.dumps(metrics))
