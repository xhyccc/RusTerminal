<template>
  <div class="flex flex-col h-screen bg-terminal-bg text-terminal-green font-mono overflow-hidden">
    <!-- Top bar -->
    <header class="flex items-center justify-between px-4 py-2 border-b border-terminal-border shrink-0">
      <div class="flex items-center gap-3">
        <span class="text-terminal-green text-lg font-bold tracking-widest">⬡ AI QUANT TERMINAL</span>
        <span class="text-xs text-terminal-dimgreen opacity-60">v0.1.0</span>
      </div>
      <div class="flex items-center gap-4 text-xs">
        <span class="flex items-center gap-1">
          <span :class="engineRunning ? 'text-terminal-green animate-pulse' : 'text-terminal-red'">●</span>
          <span>市场引擎</span>
        </span>
        <span class="text-terminal-dimgreen opacity-60">{{ heartbeatTime }}</span>
      </div>
    </header>

    <!-- Main layout -->
    <div class="flex flex-1 overflow-hidden">
      <!-- Sidebar -->
      <aside class="w-48 border-r border-terminal-border shrink-0 flex flex-col bg-terminal-panel">
        <nav class="flex flex-col gap-1 p-2 mt-2">
          <button
            v-for="tab in tabs"
            :key="tab.id"
            @click="activeTab = tab.id"
            :class="[
              'text-left px-3 py-2 text-sm rounded transition-colors duration-150',
              activeTab === tab.id
                ? 'bg-terminal-green text-black font-bold'
                : 'text-terminal-dimgreen hover:text-terminal-green hover:bg-terminal-card',
            ]"
          >
            {{ tab.icon }} {{ tab.label }}
          </button>
        </nav>
        <div class="flex-1" />
        <div class="p-3 text-xs text-terminal-dimgreen opacity-40 border-t border-terminal-border">
          <div>OPENAI GPT-4o-mini</div>
          <div>akshare · backtrader</div>
        </div>
      </aside>

      <!-- Content area -->
      <main class="flex-1 overflow-hidden flex flex-col">
        <!-- Strategy Lab -->
        <div v-if="activeTab === 'lab'" class="flex-1 flex flex-col overflow-hidden">
          <div class="flex flex-col lg:flex-row flex-1 overflow-hidden gap-0">
            <!-- Left: input + terminal -->
            <div class="flex flex-col flex-1 overflow-hidden border-r border-terminal-border">
              <StrategyInput @agent-started="onAgentStarted" @agent-done="onAgentDone" />
              <div class="border-t border-terminal-border" style="height:1px" />
              <AgentTerminal class="flex-1" />
            </div>
            <!-- Right: backtest chart -->
            <div class="w-full lg:w-96 xl:w-[480px] shrink-0 flex flex-col overflow-hidden">
              <BacktestChart />
            </div>
          </div>
        </div>

        <!-- Dashboard -->
        <div v-if="activeTab === 'dashboard'" class="flex-1 overflow-auto p-4">
          <MonitorPanel />
        </div>
      </main>
    </div>

    <!-- Status bar -->
    <footer class="border-t border-terminal-border px-4 py-1 flex items-center gap-4 text-xs text-terminal-dimgreen opacity-70 shrink-0">
      <span>{{ statusMessage }}</span>
      <span class="flex-1" />
      <span>活跃策略: {{ activeStrategyCount }}</span>
    </footer>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import StrategyInput from './components/StrategyInput.vue'
import AgentTerminal from './components/AgentTerminal.vue'
import BacktestChart from './components/BacktestChart.vue'
import MonitorPanel from './components/MonitorPanel.vue'

const tabs = [
  { id: 'lab', label: '策略研发室', icon: '⚗' },
  { id: 'dashboard', label: '监控仪表盘', icon: '📡' },
]

const activeTab = ref('lab')
const engineRunning = ref(false)
const heartbeatTime = ref('--:--:--')
const statusMessage = ref('就绪 | 等待指令…')
const activeStrategyCount = ref(0)

function onAgentStarted() {
  statusMessage.value = '⏳ Agent 正在运行…'
}

function onAgentDone(success) {
  statusMessage.value = success ? '✅ Agent 执行完成' : '❌ Agent 执行失败'
}

onMounted(async () => {
  // Start market engine
  try {
    await invoke('start_market_engine')
    engineRunning.value = true
  } catch (e) {
    console.warn('Market engine start failed (expected in browser preview):', e)
  }

  // Listen to heartbeat
  await listen('market-heartbeat', (event) => {
    engineRunning.value = true
    const d = new Date()
    heartbeatTime.value = d.toLocaleTimeString('zh-CN')
    activeStrategyCount.value = event.payload?.active_strategies ?? 0
  })
})
</script>
