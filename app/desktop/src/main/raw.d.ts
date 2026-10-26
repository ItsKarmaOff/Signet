// Files embedded as text by Vite (`import src from './file.js?raw'`).
declare module '*?raw' {
  const content: string
  export default content
}
