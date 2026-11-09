import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri dev server: 固定端口 1420，匹配 tauri.conf.json
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
