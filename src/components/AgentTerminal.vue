<template>
  <div ref="termContainer" class="w-full h-full bg-black" style="min-height: 200px;" />
</template>

<script setup>
import { ref, onMounted, onUnmounted } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { listen } from '@tauri-apps/api/event'
import '@xterm/xterm/css/xterm.css'

const termContainer = ref(null)
let term = null
let fitAddon = null
let unlisten = null
let resizeObserver = null

onMounted(async () => {
  term = new Terminal({
    theme: {
      background: '#000000',
      foreground: '#00ff41',
      cursor: '#00ff41',
      cursorAccent: '#000000',
      selectionBackground: '#00ff4133',
      black: '#000000',
      green: '#00ff41',
      brightGreen: '#39ff14',
    },
    fontFamily: '"JetBrains Mono", "Fira Code", Consolas, monospace',
    fontSize: 13,
    lineHeight: 1.4,
    cursorBlink: true,
    convertEol: true,
    scrollback: 5000,
    disableStdin: true,
  })

  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  term.open(termContainer.value)
  fitAddon.fit()

  term.writeln('\x1b[32m╔══════════════════════════════════════╗\x1b[0m')
  term.writeln('\x1b[32m║     AI QUANT TERMINAL - AGENT LOG    ║\x1b[0m')
  term.writeln('\x1b[32m╚══════════════════════════════════════╝\x1b[0m')
  term.writeln('')
  term.writeln('\x1b[90m等待 Agent 输出…\x1b[0m')

  // Listen for agent log events from Tauri backend
  try {
    unlisten = await listen('agent-log', (event) => {
      const line = event.payload ?? ''
      const colored = colorize(line)
      term.writeln(colored)
    })
  } catch (e) {
    term.writeln('\x1b[33m[仅浏览器模式] Tauri 事件不可用\x1b[0m')
  }

  resizeObserver = new ResizeObserver(() => {
    fitAddon?.fit()
  })
  resizeObserver.observe(termContainer.value)
})

onUnmounted(() => {
  unlisten?.()
  resizeObserver?.disconnect()
  term?.dispose()
})

/** Apply ANSI color codes based on log prefix. */
function colorize(line) {
  if (line.includes('[AGENT_THINKING]'))   return `\x1b[36m${line}\x1b[0m`
  if (line.includes('[AGENT_EXECUTING]'))  return `\x1b[33m${line}\x1b[0m`
  if (line.includes('[AGENT_SUCCESS]'))    return `\x1b[32m${line}\x1b[0m`
  if (line.includes('[AGENT_ERROR]'))      return `\x1b[31m${line}\x1b[0m`
  if (line.includes('[AGENT_FIXING]'))     return `\x1b[35m${line}\x1b[0m`
  return line
}
</script>
