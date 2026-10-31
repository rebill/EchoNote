import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

// Tauri serves the built files; the dev server is only for `tauri dev`.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_"],
  build: {
    // WKWebView on macOS 14.2 (Safari 17.2) and WebView2.
    target: "safari17",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
