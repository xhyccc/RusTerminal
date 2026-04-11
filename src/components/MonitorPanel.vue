<template>
  <div class="p-4">
    <div class="mb-4 flex items-center justify-between">
      <h2 class="text-sm font-bold tracking-widest text-terminal-green uppercase">
        📡 策略监控面板
      </h2>
      <button
        @click="refresh"
        class="text-xs px-3 py-1 border border-terminal-border text-terminal-dimgreen
               hover:text-terminal-green hover:border-terminal-green rounded transition-colors"
      >
        ⟳ 刷新
      </button>
    </div>

    <!-- Empty state -->
    <div
      v-if="strategies.length === 0"
      class="flex flex-col items-center justify-center py-20 text-terminal-dimgreen opacity-40 text-sm"
    >
      <div class="text-4xl mb-3">📭</div>
      <div>暂无策略，请先在策略研发室生成策略。</div>
    </div>

    <!-- Strategy grid -->
    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
      <div
        v-for="strat in strategies"
        :key="strat.id"
        class="bg-terminal-card border border-terminal-border rounded-lg p-4 flex flex-col gap-2
               hover:border-terminal-green transition-colors duration-150"
      >
        <!-- Header -->
        <div class="flex items-start justify-between gap-2">
          <p class="text-sm text-terminal-green font-bold leading-tight line-clamp-2 flex-1">
            {{ strat.idea }}
          </p>
          <!-- Toggle switch -->
          <button
            @click="toggleStrategy(strat)"
            :class="[
              'relative shrink-0 w-10 h-5 rounded-full transition-colors duration-200',
              strat.enabled ? 'bg-terminal-green' : 'bg-terminal-border',
            ]"
            :title="strat.enabled ? '点击停用' : '点击启用'"
          >
            <span
              :class="[
                'absolute top-0.5 left-0.5 w-4 h-4 bg-black rounded-full shadow transition-transform duration-200',
                strat.enabled ? 'translate-x-5' : 'translate-x-0',
              ]"
            />
          </button>
        </div>

        <!-- Metrics -->
        <div class="grid grid-cols-3 gap-1 text-xs mt-1">
          <div class="bg-black rounded p-1 text-center">
            <div class="text-terminal-dimgreen opacity-60">夏普</div>
            <div class="text-terminal-green font-bold">{{ fmt(strat.metrics?.sharpe_ratio) }}</div>
          </div>
          <div class="bg-black rounded p-1 text-center">
            <div class="text-terminal-dimgreen opacity-60">回撤</div>
            <div class="text-terminal-red font-bold">-{{ fmt(strat.metrics?.max_drawdown_pct) }}%</div>
          </div>
          <div class="bg-black rounded p-1 text-center">
            <div class="text-terminal-dimgreen opacity-60">收益</div>
            <div
              :class="(strat.metrics?.total_return_pct ?? 0) >= 0 ? 'text-terminal-green' : 'text-terminal-red'"
              class="font-bold"
            >
              {{ (strat.metrics?.total_return_pct ?? 0) >= 0 ? '+' : '' }}{{ fmt(strat.metrics?.total_return_pct) }}%
            </div>
          </div>
        </div>

        <!-- Mini sparkline -->
        <div
          v-if="strat.metrics?.equity_curve?.length"
          :ref="el => setSparklineRef(el, strat.id)"
          class="w-full h-14 mt-1"
        />

        <!-- Footer -->
        <div class="flex items-center justify-between text-xs text-terminal-dimgreen opacity-50 mt-1">
          <span>{{ formatDate(strat.timestamp) }}</span>
          <span>
            {{ strat.last_trigger ? '最后触发: ' + formatDate(strat.last_trigger) : '从未触发' }}
          </span>
        </div>
        <div class="flex items-center justify-between text-xs mt-1">
          <div class="flex items-center gap-1">
            <span :class="strat.enabled ? 'text-terminal-green' : 'text-terminal-dimgreen opacity-40'">●</span>
            <span :class="strat.enabled ? 'text-terminal-green' : 'text-terminal-dimgreen opacity-40'">
              {{ strat.enabled ? '监控中' : '已停用' }}
            </span>
          </div>
          <!-- View full chart button -->
          <button
            @click="$emit('strategy-selected', strat)"
            class="px-2 py-0.5 border border-terminal-border text-terminal-dimgreen rounded
                   hover:border-terminal-green hover:text-terminal-green transition-colors"
          >
            📈 查看回测
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, watch, nextTick, onMounted, onUnmounted } from 'vue'
import * as echarts from 'echarts'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

defineEmits(['strategy-selected'])

const strategies = ref([])
let unlisten = null
let refreshTimer = null

// ── Sparkline chart management ────────────────────────────────────────────────
const sparklineRefs = {}    // id → DOM element (populated by template ref callbacks)
const sparklineInsts = {}   // id → echarts instance

function setSparklineRef(el, id) {
  if (el) {
    sparklineRefs[id] = el
  }
}

function buildSparklineOption(curve) {
  const values = curve.map((p) => p.value)
  const isPositive = values[values.length - 1] >= values[0]
  const color = isPositive ? '#00ff41' : '#ff4141'
  return {
    animation: false,
    grid: { top: 2, right: 2, bottom: 2, left: 2 },
    xAxis: { type: 'category', show: false, data: curve.map((p) => p.date) },
    yAxis: { type: 'value', show: false, scale: true },
    series: [
      {
        data: values,
        type: 'line',
        smooth: true,
        symbol: 'none',
        lineStyle: { color, width: 1.5 },
        areaStyle: {
          color: {
            type: 'linear',
            x: 0, y: 0, x2: 0, y2: 1,
            colorStops: [
              { offset: 0, color: color.replace(')', ',0.3)').replace('rgb', 'rgba') },
              { offset: 1, color: color.replace(')', ',0.02)').replace('rgb', 'rgba') },
            ],
          },
        },
      },
    ],
  }
}

async function initSparklines() {
  await nextTick()
  for (const strat of strategies.value) {
    const curve = strat.metrics?.equity_curve
    if (!curve?.length) continue

    const el = sparklineRefs[strat.id]
    if (!el) continue

    // Dispose existing instance before re-creating to avoid stale references.
    if (sparklineInsts[strat.id]) {
      sparklineInsts[strat.id].dispose()
    }
    const inst = echarts.init(el, null, { renderer: 'canvas' })
    inst.setOption(buildSparklineOption(curve))
    sparklineInsts[strat.id] = inst
  }
}

function disposeSparklines() {
  for (const inst of Object.values(sparklineInsts)) {
    inst.dispose()
  }
  Object.keys(sparklineInsts).forEach((k) => delete sparklineInsts[k])
  Object.keys(sparklineRefs).forEach((k) => delete sparklineRefs[k])
}

// Re-init sparklines whenever strategies list updates.
watch(strategies, () => initSparklines(), { flush: 'post' })

// ── Data helpers ──────────────────────────────────────────────────────────────

function fmt(v) {
  if (v === null || v === undefined) return '--'
  return Number(v).toFixed(2)
}

function formatDate(iso) {
  if (!iso) return '--'
  try {
    return new Date(iso).toLocaleString('zh-CN', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return iso
  }
}

async function refresh() {
  try {
    strategies.value = await invoke('get_strategies')
  } catch {
    // Browser preview fallback: try reading file directly
    try {
      const { readTextFile } = await import('@tauri-apps/plugin-fs')
      const raw = await readTextFile('python_agent/workspace/strategies.json')
      strategies.value = JSON.parse(raw)
    } catch {
      strategies.value = []
    }
  }
}

async function toggleStrategy(strat) {
  strat.enabled = !strat.enabled
  try {
    await invoke('toggle_strategy', { id: strat.id, enabled: strat.enabled })
  } catch (e) {
    console.warn('toggle_strategy not available in browser mode:', e)
  }
}

onMounted(async () => {
  await refresh()

  // Refresh when agent completes
  try {
    unlisten = await listen('agent-log', async (event) => {
      if (typeof event.payload === 'string' && event.payload.includes('[AGENT_SUCCESS]')) {
        await refresh()
      }
    })
  } catch { /* browser mode */ }

  // Poll every 30 seconds
  refreshTimer = setInterval(refresh, 30000)
})

onUnmounted(() => {
  unlisten?.()
  clearInterval(refreshTimer)
  disposeSparklines()
})
</script>
