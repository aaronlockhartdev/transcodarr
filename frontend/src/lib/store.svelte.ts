// Shared server state. A module-level Svelte 5 state class: components import
// `store` and get the same reactive instance. Polling cadence is deliberately
// gentle — this is a LAN self-hosted tool, not a trading desk.

import { api } from "./api.js";
import { sse } from "./events.svelte.js";
import type { Device, FileRow, FlowRecord, Health, JobRow, Library } from "./types.js";

const POLL_MS = 5000;
// A connected stream that goes silent for longer than this (two missed
// 30 s ticks) is treated as dead — the poll fallback resumes even
// though EventSource still reports itself open.
const SSE_STALE_MS = 60_000;

class Store {
	libraries = $state<Library[]>([]);
	// Plain state Record (NOT a Map): a $state Map does not reliably notify
	// deriveds that first read a key while it was absent (a hard page reload
	// starts with an empty map, so the derived evaluates against missing keys
	// and later in-place .set() updates never re-render it). Property-level
	// state on an object proxy is the canonical reliable pattern, and the
	// per-poll writes below are plain property sets.
	filesByLibrary = $state<Record<number, FileRow[]>>({});
	jobs = $state<JobRow[]>([]);
	devices = $state<Device[]>([]);
	flows = $state<FlowRecord[]>([]);
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
			const [libs, flows, devs, health] = await Promise.all([
				api.listLibraries(),
				api.listFlows(),
				api.devices(),
				api.health(),
			]);
			this.libraries = libs;
			this.flows = flows;
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
				this.filesByLibrary[lib.id] = files;
			}),
		);
		// A library that vanished mid-flight: drop its cache entry.
		for (const id of Object.keys(this.filesByLibrary)) {
			if (!this.libraries.some((l) => l.id === Number(id))) {
				delete this.filesByLibrary[Number(id)];
			}
		}
		return results;
	}

	async refreshLibraryFiles(libraryId: number) {
		try {
			this.filesByLibrary[libraryId] = await api.listFiles(libraryId);
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
			// The SSE stream drives updates; the poll is the fallback.
			// "Up" means connected AND actually receiving events — an
			// open-but-silent stream (dropped without a close event, a
			// wedged proxy) would otherwise disable the fallback
			// forever and let rows go stale indefinitely.
			const sseHealthy = sse.connected && Date.now() - sse.lastEventAt < SSE_STALE_MS;
			if (sseHealthy) return;
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
		return this.filesByLibrary[libraryId] ?? [];
	}
}

export const store = new Store();
