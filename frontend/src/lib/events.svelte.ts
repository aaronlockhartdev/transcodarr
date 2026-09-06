// The live-update client (DESIGN §9.0): one EventSource to /api/events.
//
// EventSource reconnects automatically and re-sends the last seen
// event id as `Last-Event-ID`; the server replays its in-memory ring
// before joining the live feed — so a dropped connection (or a page
// load mid-job) resumes without losing events. If a connected client
// falls behind the channel's capacity, the server sends a `resync`
// frame and we full-refetch instead of guessing. While the stream is
// up, the ambient 5 s poll is skipped; it remains the disconnect
// fallback (see `Store.startPolling`).

import { store } from "./store.svelte.js";

type LogListener = (chunk: string) => void;

let es: EventSource | null = null;
const logListeners = new Map<number, Set<LogListener>>();
const debounced = new Map<string, ReturnType<typeof setTimeout>>();

/** Stream liveness. `connected` is true between onopen and the next
 * onerror; `lastEventAt` moves on every frame, including ticks. */
export const sse = $state({ connected: false, lastEventAt: 0 });

function debouncedUpdate(key: string, fn: () => void, ms = 400) {
	const existing = debounced.get(key);
	if (existing) clearTimeout(existing);
	debounced.set(
		key,
		setTimeout(() => {
			debounced.delete(key);
			fn();
		}, ms),
	);
}

function parsed(e: Event): Record<string, unknown> | null {
	const data = (e as MessageEvent).data;
	if (!data) return null;
	try {
		return JSON.parse(data);
	} catch {
		return null;
	}
}

/** Start the stream (idempotent). */
export function startEvents() {
	if (es) return;
	es = new EventSource("/api/events");
	es.onopen = () => {
		sse.connected = true;
	};
	es.onerror = () => {
		// EventSource reconnects by itself; treat the gap as down so
		// the poll fallback picks up.
		sse.connected = false;
	};
	es.addEventListener("job_changed", (e) => {
				sse.lastEventAt = Date.now();
		// A nudge: refetch the full job list (cheap on a LAN).
		debouncedUpdate("jobs", () => void store.refreshJobs());
	});
	es.addEventListener("job_log", (e) => {
				sse.lastEventAt = Date.now();
		const ev = parsed(e) as { job_id?: number; chunk?: string } | null;
		if (!ev || typeof ev.job_id !== "number" || typeof ev.chunk !== "string") return;
		const set = logListeners.get(ev.job_id);
		if (set) for (const fn of set) fn(ev.chunk);
	});
	es.addEventListener("file_changed", (e) => {
				sse.lastEventAt = Date.now();
		const ev = parsed(e) as { library_id?: number } | null;
		if (!ev || typeof ev.library_id !== "number") return;
		const libId = ev.library_id;
		debouncedUpdate(`lib:${libId}`, () => void store.refreshLibraryFiles(libId));
	});
	es.addEventListener("library_changed", () => {
				sse.lastEventAt = Date.now();
		debouncedUpdate("core", () => void store.refreshCore());
	});
	es.addEventListener("flow_changed", () => {
				sse.lastEventAt = Date.now();
		debouncedUpdate("core", () => void store.refreshCore());
	});
	es.addEventListener("tick", () => {
		sse.lastEventAt = Date.now();
	});
	es.addEventListener("resync", () => {
		// The bus moved on without us: full refetch, both sides.
		void store.refreshCore();
		void store.refreshJobs();
	});
}

export function stopEvents() {
	es?.close();
	es = null;
	sse.connected = false;
}

/** Subscribe to a job's live log chunks; returns an unsubscribe. */
export function subscribeJobLog(jobId: number, fn: LogListener): () => void {
	let set = logListeners.get(jobId);
	if (!set) {
		set = new Set();
		logListeners.set(jobId, set);
	}
	set.add(fn);
	return () => {
		set.delete(fn);
		if (set.size === 0) logListeners.delete(jobId);
	};
}
