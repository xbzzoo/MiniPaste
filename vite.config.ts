import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri 期望固定端口；src-tauri 由 cargo 自己监听，不需要 vite 盯着
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
