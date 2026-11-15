// `npx vite --config vite.shots.config.ts`: the UI with a fake backend, for website screenshots only.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

const shot = (f: string) => fileURLToPath(new URL(`./shots/${f}`, import.meta.url));
export default defineConfig({
  plugins: [react()],
  server: { port: 1430, strictPort: true },
  resolve: {
    alias: {
      "@tauri-apps/api/core": shot("core.ts"),
      "@tauri-apps/api/event": shot("event.ts"),
      "@tauri-apps/api/webviewWindow": shot("window.ts"),
    },
  },
});
