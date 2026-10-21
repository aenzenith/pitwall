import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [vue()],

  // The Files window's colouring worker loads each language's grammar as a chunk of its own.
  worker: { format: "es" as const },

  // The Markdown preview's libraries, loaded with the first preview: bundled ahead in dev, so
  // that first preview doesn't reload the page while Vite finds them.
  optimizeDeps: {
    include: ["markdown-it", "markdown-it-anchor", "markdown-it-emoji", "markdown-it-footnote", "markdown-it-front-matter", "markdown-it-github-alerts", "markdown-it-task-lists", "@vscode/markdown-it-katex", "katex", "dompurify", "mermaid"],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
