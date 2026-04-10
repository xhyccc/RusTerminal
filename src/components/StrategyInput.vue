<template>
  <div class="p-4 border-b border-terminal-border bg-terminal-panel shrink-0">
    <div class="mb-2 text-xs text-terminal-dimgreen opacity-70 tracking-wider uppercase">
      ⚗ 策略研发 — 输入你的量化灵感
    </div>
    <textarea
      v-model="idea"
      :disabled="loading"
      rows="4"
      placeholder="输入你的量化灵感，例如：5日均线与20日均线金叉策略，标的为平安银行 000001"
      class="w-full bg-black border border-terminal-border text-terminal-green placeholder-terminal-dimgreen
             placeholder-opacity-40 font-mono text-sm px-3 py-2 rounded resize-none outline-none
             focus:border-terminal-green transition-colors duration-150
             disabled:opacity-50 disabled:cursor-not-allowed"
    />
    <div class="flex items-center gap-3 mt-2">
      <button
        @click="startAgent"
        :disabled="loading || !idea.trim()"
        class="px-5 py-2 text-sm font-bold rounded border transition-all duration-150
               border-terminal-green text-black bg-terminal-green
               hover:bg-terminal-dimgreen hover:border-terminal-dimgreen
               disabled:opacity-40 disabled:cursor-not-allowed"
      >
        <span v-if="!loading">▶ 开始研发</span>
        <span v-else class="flex items-center gap-1">
          <span class="animate-spin inline-block">⟳</span> 运行中{{ dots }}
        </span>
      </button>
      <span v-if="statusText" :class="statusClass" class="text-xs">{{ statusText }}</span>
    </div>
  </div>
</template>

<script setup>
import { ref, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const emit = defineEmits(['agent-started', 'agent-done'])

const idea = ref('')
const loading = ref(false)
const statusText = ref('')
const statusClass = ref('text-terminal-dimgreen')
const dots = ref('')

let dotsInterval = null

function startDots() {
  let n = 0
  dotsInterval = setInterval(() => {
    dots.value = '.'.repeat((n % 3) + 1)
    n++
  }, 500)
}

function stopDots() {
  clearInterval(dotsInterval)
  dots.value = ''
}

async function startAgent() {
  if (!idea.value.trim() || loading.value) return

  loading.value = true
  statusText.value = '正在启动 AI Agent…'
  statusClass.value = 'text-terminal-dimgreen'
  emit('agent-started')
  startDots()

  try {
    await invoke('run_agent', { idea: idea.value.trim() })
    statusText.value = '✅ 策略生成完成'
    statusClass.value = 'text-terminal-green'
    emit('agent-done', true)
  } catch (err) {
    statusText.value = `❌ ${err}`
    statusClass.value = 'text-terminal-red'
    emit('agent-done', false)
  } finally {
    loading.value = false
    stopDots()
  }
}

onUnmounted(() => stopDots())
</script>
