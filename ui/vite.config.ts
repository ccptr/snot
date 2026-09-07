import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  // Tauri prints its own diagnostics; don't wipe them off the terminal.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 5183 } : undefined,
    watch: { ignored: ["**/target/**", "**/gen/**"] },
  },
  build: {
    // The oldest webviews Tauri ships against: WKWebView on macOS/iOS,
    // WebView2 on Windows, WebKitGTK on Linux, Chromium on Android.
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari15",
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
