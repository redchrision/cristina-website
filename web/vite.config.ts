import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    // content/*.json lives one level up and is shared with the Rust server
    fs: { allow: [".."] },
    // `npm run dev` talks to the Rust server for /api and /v1
    proxy: {
      "/api": "http://127.0.0.1:8080",
      "/v1": "http://127.0.0.1:8080",
    },
  },
});
