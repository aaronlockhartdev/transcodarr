import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";

// The Rust server embeds the built assets (see crates/server/build.rs), so in
// production there is no dev-server concept. In development, /api is proxied
// to a locally running `transcodarr-server` so the SPA talks to the real API.
export default defineConfig({
	plugins: [svelte(), tailwindcss()],
	resolve: {
		alias: {
			$lib: fileURLToPath(new URL("./src/lib", import.meta.url)),
			$routes: fileURLToPath(new URL("./src/routes", import.meta.url)),
		},
	},
	// vite 8 (rolldown) pre-bundles ESM deps too, which splits svelte's
	// multi-entry internal runtime into duplicate module instances and
	// breaks its media-query event subscription at runtime. Every dep in
	// this project is ESM, so disable dev pre-bundling entirely and let
	// the browser load node_modules ESM directly (single svelte instance).
	optimizeDeps: {
		exclude: [
			'svelte',
			'svelte/animate',
			'svelte/attachments',
			'svelte/easing',
			'svelte/events',
			'svelte/internal',
			'svelte/internal/client',
			'svelte/internal/disclose-version',
			'svelte/internal/flags/async',
			'svelte/internal/flags/legacy',
			'svelte/internal/flags/tracing',
			'svelte/legacy',
			'svelte/motion',
			'svelte/reactivity',
			'svelte/reactivity/window',
			'svelte/store',
			'svelte/transition',
		],
	},
	server: {
		port: 5173,
		strictPort: true,
		proxy: {
			"/api": {
				target: "http://127.0.0.1:8481",
				changeOrigin: true,
			},
		},
	},
});
