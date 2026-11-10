import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// The dev server serves the webview only during `tauri dev`, on loopback.
// The built app loads its files from disk and binds no port.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { host: "127.0.0.1", port: 5173, strictPort: true },
  build: { outDir: "dist", target: "es2022" },
  // `css.include` makes Vitest load CSS, so theme.test.ts reads the real `?raw` text.
  test: { environment: "node", css: { include: [/.+/] } },
});
