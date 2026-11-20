import { defineConfig } from "vitest/config";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;

// Mirrors Rust `u16::from_str` after trim(): optional leading '+', decimal digits
// only (leading zeros allowed), filtered to 1..=65535. `Number()` alone would
// accept "47911.0", "1e4" and "0x1F", which Rust rejects, so the desktop shell
// and the dev proxy could end up on different ports.
export function parsePort(raw: string | undefined): number | undefined {
  if (raw === undefined) return undefined;
  const value = raw.trim();
  if (!/^\+?\d+$/.test(value)) return undefined;
  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed >= 1 && parsed <= 65535 ? parsed : undefined;
}

// Same env names as N.E.K.O's config/network.py.
export function port(name: string, fallback: number): number {
  return (
    [`NEKO_${name}`, name]
      .map((key) => parsePort(process.env[key]))
      .find((value) => value !== undefined) ?? fallback
  );
}

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
