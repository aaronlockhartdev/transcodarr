import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';

export default {
	plugins: [
		sveltekit({
			adapter: adapter({ fallback: 'index.html' })
		})
	],
	server: {
		proxy: { '/api': 'http://127.0.0.1:8080' }
	}
};
