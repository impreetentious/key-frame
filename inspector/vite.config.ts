import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The site is served from a project page, so every asset URL is relative.
// `base: "./"` keeps the build working whether it is opened from the deployed
// path, a preview server, or a file:// checkout during review.
export default defineConfig({
  base: "./",
  plugins: [react()],
  build: {
    outDir: "dist",
    assetsInlineLimit: 0,
    // The decoder is fetched, not bundled: it is a WebAssembly module, and
    // inlining it as base64 would inflate the JavaScript budget by a third of
    // a megabyte to save one request.
    rollupOptions: {
      output: {
        assetFileNames: "assets/[name]-[hash][extname]",
      },
    },
  },
});
