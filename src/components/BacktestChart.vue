<template>
  <div class="flex flex-col h-full bg-terminal-panel p-4 overflow-hidden">
    <div class="mb-2 text-xs text-terminal-dimgreen opacity-70 tracking-wider uppercase shrink-0">
      📈 回测结果
    </div>

    <!-- Metrics table -->
    <div v-if="metrics" class="grid grid-cols-2 gap-2 mb-3 shrink-0">
      <div class="bg-terminal-card rounded p-2 border border-terminal-border">
        <div class="text-xs text-terminal-dimgreen opacity-60">夏普比率</div>
        <div :class="metricClass(metrics.sharpe_ratio, 1, 0)" class="text-lg font-bold">
          {{ fmt(metrics.sharpe_ratio) }}
        </div>
      </div>
      <div class="bg-terminal-card rounded p-2 border border-terminal-border">
        <div class="text-xs text-terminal-dimgreen opacity-60">最大回撤</div>
        <div class="text-lg font-bold text-terminal-red">
          -{{ fmt(metrics.max_drawdown_pct) }}%
        </div>
      </div>
      <div class="bg-terminal-card rounded p-2 border border-terminal-border">
        <div class="text-xs text-terminal-dimgreen opacity-60">总收益率</div>
        <div :class="metricClass(metrics.total_return_pct, 0, 0)" class="text-lg font-bold">
          {{ metrics.total_return_pct >= 0 ? '+' : '' }}{{ fmt(metrics.total_return_pct) }}%
        </div>
      </div>
      <div class="bg-terminal-card rounded p-2 border border-terminal-border">
        <div class="text-xs text-terminal-dimgreen opacity-60">交易次数</div>
        <div class="text-lg font-bold text-terminal-green">{{ metrics.num_trades ?? '--' }}</div>
      </div>
    </div>
    <div v-else class="text-xs text-terminal-dimgreen opacity-40 mb-3 shrink-0">
      尚无回测数据，运行策略后自动更新…
    </div>

    <!-- ECharts equity curve -->
    <div ref="chartEl" class="flex-1 min-h-0 rounded border border-terminal-border" />
  </div>
</template>

<script setup>
import { ref, watch, onMounted, onUnmounted } from 'vue'
import * as echarts from 'echarts'
import { listen } from '@tauri-apps/api/event'

const props = defineProps({
  /** When set, display this strategy instead of the latest report.json. */
  selectedStrategy: { type: Object, default: null },
})

const chartEl = ref(null)
const metrics = ref(null)
const equityCurve = ref([])
const chartTitle = ref('回测结果')

let chart = null
let unlisten = null
let resizeObserver = null

function fmt(v) {
  if (v === null || v === undefined) return '--'
  return Number(v).toFixed(2)
}

function metricClass(val, good, bad) {
  if (val === null || val === undefined) return 'text-terminal-dimgreen'
  if (val >= good) return 'text-terminal-green'
  if (val <= bad) return 'text-terminal-red'
  return 'text-terminal-amber'
}

function buildOption(curve) {
  const dates = curve.map((p) => p.date)
  const values = curve.map((p) => p.value)

  return {
    backgroundColor: 'transparent',
    grid: { top: 20, right: 16, bottom: 40, left: 56 },
    tooltip: {
      trigger: 'axis',
      backgroundColor: '#0d1a0d',
      borderColor: '#00ff41',
      textStyle: { color: '#00ff41', fontFamily: 'monospace', fontSize: 12 },
      formatter: (params) => {
        const p = params[0]
        return `${p.axisValue}<br/>¥${Number(p.value).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`
      },
    },
    xAxis: {
      type: 'category',
      data: dates,
      axisLine: { lineStyle: { color: '#1a2a1a' } },
      axisLabel: { color: '#00cc33', fontSize: 10 },
      splitLine: { show: false },
    },
    yAxis: {
      type: 'value',
      axisLine: { lineStyle: { color: '#1a2a1a' } },
      axisLabel: { color: '#00cc33', fontSize: 10 },
      splitLine: { lineStyle: { color: '#1a2a1a', type: 'dashed' } },
    },
    series: [
      {
        data: values,
        type: 'line',
        smooth: true,
        symbol: 'none',
        lineStyle: { color: '#00ff41', width: 2 },
        areaStyle: {
          color: {
            type: 'linear',
            x: 0, y: 0, x2: 0, y2: 1,
            colorStops: [
              { offset: 0, color: 'rgba(0,255,65,0.25)' },
              { offset: 1, color: 'rgba(0,255,65,0.01)' },
            ],
          },
        },
      },
    ],
  }
}

async function loadReport() {
  try {
    // Try to read via Tauri fs
    const { readTextFile } = await import('@tauri-apps/plugin-fs')
    const raw = await readTextFile('python_agent/workspace/report.json')
    const report = JSON.parse(raw)
    metrics.value = report.metrics
    if (report.metrics?.equity_curve) {
      equityCurve.value = report.metrics.equity_curve
      chart?.setOption(buildOption(equityCurve.value))
    }
  } catch {
    // Fallback: demo data for browser preview
    const demo = Array.from({ length: 60 }, (_, i) => ({
      date: `2023-${String(Math.floor(i / 5) + 1).padStart(2, '0')}-${String((i % 5) * 5 + 1).padStart(2, '0')}`,
      value: 100000 + Math.sin(i * 0.3) * 5000 + i * 300,
    }))
    equityCurve.value = demo
    chart?.setOption(buildOption(demo))
  }
}

onMounted(async () => {
  chart = echarts.init(chartEl.value, null, { renderer: 'canvas' })
  chart.setOption(buildOption([]))

  resizeObserver = new ResizeObserver(() => chart?.resize())
  resizeObserver.observe(chartEl.value)

  await loadReport()

  try {
    unlisten = await listen('agent-log', async (event) => {
      if (typeof event.payload === 'string' && event.payload.includes('[AGENT_SUCCESS]')) {
        await loadReport()
      }
    })
  } catch { /* browser mode */ }
})

onUnmounted(() => {
  unlisten?.()
  resizeObserver?.disconnect()
  chart?.dispose()
})
</script>
