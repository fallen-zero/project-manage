import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error 不装 @types/node 时这里会报类型缺失
import process from "node:process";
import { fileURLToPath, URL } from "node:url";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    // 不用 Tauri 默认的 1420：本机 Windows 把 1397-1496 整段列为 TCP 排除端口（Hyper-V/WSL 保留），
    // 监听会直接 EACCES。换 1520 后需与 tauri.conf.json 的 devUrl 保持一致。
    port: 1520,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1521 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
