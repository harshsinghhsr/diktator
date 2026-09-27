import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

// @ts-expect-error process is a Node global; @types/node is not installed
const host: string | undefined = process.env.TAURI_DEV_HOST;
const root = import.meta.dirname;

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
  build: {
    rollupOptions: {
      input: {
        settings: resolve(root, "settings.html"),
        overlay: resolve(root, "overlay.html"),
      },
    },
  },
});
