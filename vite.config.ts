import { defineConfig } from "vitest/config";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;
// Same env names as N.E.K.O's config/network.py.
const port = (name: string, fallback: number) =>
  [`NEKO_${name}`, name]
    .map((key) => Number(process.env[key]))
    .find((value) => Number.isInteger(value) && value >= 1 && value <= 65535) ?? fallback;

export default defineConfig(async () => ({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    proxy: {
      '/memory': {
        target: `http://127.0.0.1:${port('MEMORY_SERVER_PORT', 48912)}`,
        changeOrigin: true,
      },
      '/agent': {
        target: `http://127.0.0.1:${port('TOOL_SERVER_PORT', 48915)}`,
        changeOrigin: true,
      },
      '/api': {
        target: `http://127.0.0.1:${port('MAIN_SERVER_PORT', 48911)}`,
        changeOrigin: true,
      },
      '/ws': {
        target: `http://127.0.0.1:${port('MAIN_SERVER_PORT', 48911)}`,
        ws: true,
      }
    },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  test: {
    globals: true,
    environment: "jsdom",
  },
}));
