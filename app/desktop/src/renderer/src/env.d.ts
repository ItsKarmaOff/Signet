/// <reference types="vite/client" />

interface Window {
  /** Glisser du mini-lecteur (preload). */
  signetPip?: { drag(on: boolean): void; resize(phase: 'start' | 'move' | 'end', handle?: string, dx?: number, dy?: number): void }
}
