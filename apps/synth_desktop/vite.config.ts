import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
	root: resolve("src/renderer"),
	// Parallel Playwright workers each own a Vite server. Their dependency
	// optimizer state must not share a cache; the fixture supplies a private
	// directory while normal development keeps Vite's default cache.
	cacheDir: process.env.SYNTH_DESKTOP_VITE_CACHE_DIR || undefined,
	resolve: {
		alias: {
			"@": resolve("src/renderer/src"),
			"@synth/visuals": resolve("../../visuals/registry/index.ts")
		}
	},
	plugins: [react()],
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true
	},
	build: {
		outDir: resolve("dist"),
		emptyOutDir: true
	}
});
