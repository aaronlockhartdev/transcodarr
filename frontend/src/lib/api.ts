import type {
	Device,
	FileRow,
	FlowRecord,
	FlowSchema,
	Health,
	JobRow,
	Library,
} from "./types.js";

export class ApiError extends Error {
	constructor(
		public status: number,
		message: string,
	) {
		super(message);
	}
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
	const res = await fetch(path, {
		headers: { "content-type": "application/json" },
		...init,
	});
	if (res.status === 204) return undefined as T;
	let body: unknown = null;
	const ct = res.headers.get("content-type") ?? "";
	if (ct.includes("application/json")) {
		try {
			body = await res.json();
		} catch {
			// non-JSON error body
		}
	} else if (ct.includes("text/plain")) {
		body = await res.text();
	}
	if (!res.ok) {
		const msg =
			body && typeof body === "object" && "error" in (body as Record<string, unknown>)
				? String((body as Record<string, unknown>).error)
				: `${res.status} ${res.statusText}`;
		throw new ApiError(res.status, msg);
	}
	return body as T;
}

/** The typed API client. All paths are same-origin (vite proxies /api in
 *  development; the Rust binary serves both in production). */
export const api = {
	health: () => request<Health>("/api/health"),
	schemaFlow: () => request<FlowSchema>("/api/schema/flow"),
	devices: () => request<Device[]>("/api/devices"),

	listFlows: () => request<FlowRecord[]>("/api/flows"),
	getFlow: (id: number) => request<FlowRecord>(`/api/flows/${id}`),
	createFlow: (body: { name: string; flow_json: string }) =>
		request<{ id: number }>("/api/flows", {
			method: "POST",
			body: JSON.stringify(body),
		}),
	updateFlow: (id: number, body: { name: string; flow_json: string }) =>
		request<void>(`/api/flows/${id}`, {
			method: "PUT",
			body: JSON.stringify(body),
		}),
	deleteFlow: (id: number) => request<void>(`/api/flows/${id}`, { method: "DELETE" }),

	listLibraries: () => request<Library[]>("/api/libraries"),
	getLibrary: (id: number) => request<Library>(`/api/libraries/${id}`),
	/** The server re-evaluates all libraries that use the assigned flow. */
	createLibrary: (lib: Omit<Library, "id" | "flow_name">) =>
		request<{ id: number }>("/api/libraries", {
			method: "POST",
			body: JSON.stringify(lib),
		}),
	updateLibrary: (lib: Omit<Library, "flow_name">) =>
		request<void>(`/api/libraries/${lib.id}`, {
			method: "PUT",
			body: JSON.stringify(lib),
		}),
	deleteLibrary: (id: number) =>
		request<void>(`/api/libraries/${id}`, { method: "DELETE" }),

	listFiles: (libraryId: number) => request<FileRow[]>(`/api/libraries/${libraryId}/files`),
	scanLibrary: (libraryId: number) =>
		request<{ scanned: number; queued: number }>(`/api/libraries/${libraryId}/scan`, {
			method: "POST",
		}),

	listJobs: (limit = 500) => request<JobRow[]>(`/api/jobs?limit=${limit}`),
	getJob: (id: number) => request<JobRow>(`/api/jobs/${id}`),
	jobLog: (id: number) => request<string>(`/api/jobs/${id}/log`),

	getSetting: (key: string) => request<{ key: string; value: string | null }>(`/api/settings/${encodeURIComponent(key)}`),
	setSetting: (key: string, value: string) =>
		request<void>(`/api/settings/${encodeURIComponent(key)}`, {
			method: "PUT",
			body: JSON.stringify({ value }),
		}),
};
