import { defineConfig } from "vitest/config";
import solid from "vite-plugin-solid";

// Tauri espera un puerto fijo en desarrollo y no queremos que limpie la pantalla.
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", outDir: "dist" },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    server: { deps: { inline: [/solid-js/] } },
  },
  resolve: { conditions: ["development", "browser"] },
});
