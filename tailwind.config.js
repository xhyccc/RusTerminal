/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{vue,js,ts,jsx,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        terminal: {
          bg: '#0a0a0a',
          green: '#00ff41',
          dimgreen: '#00cc33',
          amber: '#ffb000',
          red: '#ff2020',
          border: '#1a2a1a',
          panel: '#0d1a0d',
          card: '#0f1f0f',
        },
      },
      fontFamily: {
        mono: ['"JetBrains Mono"', '"Fira Code"', 'Consolas', 'monospace'],
      },
    },
  },
  plugins: [],
}
