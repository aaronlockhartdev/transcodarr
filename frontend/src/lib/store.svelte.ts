// Shared server state. A module-level Svelte 5 state class: components import
// `store` and get the same reactive instance. Polling cadence is deliberately
// gentle — this is a LAN self-hosted tool, not a trading desk.

import { api } from "./api.js";
import type { Device, FileRow, Health, JobRow, Library } from "./types.js";

const POLL_MS = 5000;

class Store {
	libraries = $state<Library[]>([]);
	filesByLibrary = $state(new Map<number, FileRow[]>());
	jobs = $state<JobRow[]>([]);
	devices = $state<Device[]>([]);
	health = $state<Health | null>(null);
	/** Non-null while a refresh is in flight (drives skeletons). */
	refreshing = $state(false);
	/** Last surfaced fetch error (toast text); null when healthy. */
	fetchError = $state<string | null>(null);

	private timer: ReturnType<typeof setInterval> | null = null;

	async refreshCore() {
		if (this.refreshing) return; // stale-response guard: never overlap in-flight refreshes
		this.refreshing = true;
		try {
			const [libs, devs, health] = await Promise.all([
				api.listLibraries(),
				api.devices(),
				api.health(),
			]);
			this.libraries = libs;
			this.devices = devs;
			this.health = health;
			this.fetchError = null;
			await this.refreshAllFiles();
		} catch (e) {
			this.fetchError = e instanceof Error ? e.message : String(e);
		} finally {
			this.refreshing = false;
		}
	}

	async refreshAllFiles() {
		const results = await Promise.allSettled(
			this.libraries.map(async (lib) => {
				const files = await api.listFiles(lib.id);
				this.filesByLibrary.set(lib.id, files);
			}),
		);
		// A library that vanished mid-flight: drop its cache entry.
		for (const [id] of this.filesByLibrary) {
			if (!this.libraries.some((l) => l.id === id)) {
				this.filesByLibrary.delete(id);
			}
		}
		return results;
	}

	async refreshLibraryFiles(libraryId: number) {
		try {
			this.filesByLibrary.set(libraryId, await api.listFiles(libraryId));
		} catch {
			// transient; the next poll retries
		}
	}

	async refreshJobs() {
		try {
			this.jobs = await api.listJobs(500);
		} catch {
			// transient
		}
	}

	/** Start the ambient poll loop (idempotent). */
	startPolling() {
		if (this.timer) return;
		void this.refreshCore();
		void this.refreshJobs();
		this.timer = setInterval(() => {
			void this.refreshCore();
			void this.refreshJobs();
		}, POLL_MS);
	}

	stopPolling() {
		if (this.timer) {
			clearInterval(this.timer);
			this.timer = null;
		}
	}

	filesOf(libraryId: number): FileRow[] {
		return this.filesByLibrary.get(libraryId) ?? [];
	}
}

export const store = new Store();
