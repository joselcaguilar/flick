import { resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    fs: {
      allow: [resolve(import.meta.dirname, "..")],
    },
  },
  preview: {
    port: 4173,
    host: "127.0.0.1",
  },
  build: {
    rollupOptions: {
      input: {
        app: resolve(import.meta.dirname, "index.html"),
        hud: resolve(import.meta.dirname, "hud.html"),
      },
    },
  },
});
