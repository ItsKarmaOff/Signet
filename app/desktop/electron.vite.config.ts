import { resolve } from 'node:path'
import { defineConfig } from 'electron-vite'
import react from '@vitejs/plugin-react'
import { emojiPlugin } from './scripts/emoji-plugin'

const shared = { alias: { '@shared': resolve(__dirname, 'src/shared') } }

export default defineConfig({
  main: { resolve: shared },
  // The sandboxed preload cannot `require` modules: the library code is bundled into it.
  preload: {
    resolve: shared,
    build: {
      externalizeDeps: { exclude: ['electron-chrome-extensions'] },
      // Standalone page scripts: identity, drag to navigate, passwords, webauthn, notes.
      rollupOptions: {
        input: {
          index: resolve(__dirname, 'src/preload/index.ts'),
          identity: resolve(__dirname, 'src/preload/identity.ts'),
          swipe: resolve(__dirname, 'src/preload/swipe.ts'),
          passwords: resolve(__dirname, 'src/preload/passwords.ts'),
          webauthn: resolve(__dirname, 'src/preload/webauthn.ts'),
          notes: resolve(__dirname, 'src/preload/notes.ts')
        }
      }
    }
  },
  // Minified UI (JS and CSS); main and preload stay readable for error traces.
  // No Vite error overlay in development: it crashes in the transparent views and would cover the interface
  // (compile errors stay in the terminal).
  renderer: {
    resolve: shared,
    plugins: [react(), emojiPlugin(__dirname)],
    server: { hmr: { overlay: false } },
    build: { minify: true, cssMinify: true }
  }
})
