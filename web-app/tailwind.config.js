/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ['class', '[data-theme="dark"]'],
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        bg: 'rgb(var(--bg) / <alpha-value>)',
        surface: 'rgb(var(--surface) / <alpha-value>)',
        surface2: 'rgb(var(--surface-2) / <alpha-value>)',
        line: 'rgb(var(--line) / <alpha-value>)',
        ink: 'rgb(var(--text) / <alpha-value>)',
        ink2: 'rgb(var(--text-2) / <alpha-value>)',
        ink3: 'rgb(var(--text-3) / <alpha-value>)',
        accent: 'rgb(var(--accent) / <alpha-value>)',
        ok: 'rgb(var(--ok) / <alpha-value>)',
        warn: 'rgb(var(--warn) / <alpha-value>)',
        danger: 'rgb(var(--danger) / <alpha-value>)',
        violet: 'rgb(var(--violet) / <alpha-value>)',
        cyan: 'rgb(var(--cyan) / <alpha-value>)',
        teal: 'rgb(var(--teal) / <alpha-value>)',
        amber: 'rgb(var(--amber) / <alpha-value>)',
        rose: 'rgb(var(--rose) / <alpha-value>)',
        indigo: 'rgb(var(--indigo) / <alpha-value>)',
        // The active section's hue. Set once by the shell, so a component can
        // pick it up without knowing which section it is rendered in — and it
        // stays a static class name, which Tailwind's scanner requires.
        section: 'rgb(var(--section) / <alpha-value>)',
      },
      borderRadius: {
        xl: '12px',
        lg: '10px',
      },
      maxWidth: {
        shell: '72rem',
      },
    },
  },
  plugins: [],
}
