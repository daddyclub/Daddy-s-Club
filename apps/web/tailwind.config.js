/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      fontFamily: {
        mono: ['"JetBrains Mono"', 'ui-monospace', 'SFMono-Regular', 'Menlo', 'monospace'],
      },
      colors: {
        board: {
          bg: 'hsl(var(--board-bg))',
          panel: 'hsl(var(--board-panel))',
          cell: 'hsl(var(--board-cell))',
          ink: 'hsl(var(--board-ink))',
          dim: 'hsl(var(--board-dim))',
          faint: 'hsl(var(--board-faint))',
          rule: 'hsl(var(--board-rule))',
          accent: 'hsl(var(--board-accent))',
          red: 'hsl(var(--board-red))',
        },
      },
      borderRadius: { lg: '0px', md: '0px', sm: '0px' },
    },
  },
  plugins: [],
};
