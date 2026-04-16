import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const currentDir = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  base: "./",
  envPrefix: "VITE_",
  plugins: [tailwindcss(), react()],
  root: "src",
  resolve: {
    alias: {
      "@": path.resolve(currentDir, "./src"),
    },
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    minify: "esbuild",
    sourcemap: false,
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
});
