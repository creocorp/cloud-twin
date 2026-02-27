import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The CloudTwin API runs on port 4793.
// In dev, Vite proxies /api/* to the Python backend so you can run:
//   python -m cloudtwin &
//   cd dashboard && npm run dev
// In production the FastAPI app mounts the built dist/ as StaticFiles on port 8787.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 8787,
    proxy: {
      "/api": {
        target: "http://localhost:4793",
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: "dist",
  },
});
