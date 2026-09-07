// TypeScript mirrors of the transcodarr-server API (crates/server/src/db.rs
// row types and the /api/schema/flow payload). Keep in sync with the Rust
// serde shapes.

export interface DeviceEncoder {
	name: string;
	target: string;
}

export interface Device {
	id: string;
	kind: "cpu" | "gpu";
	name: string;
	max_concurrent: number;
	encoders: DeviceEncoder[];
}

export interface Library {
	id: number;
	name: string;
	path: string;
	/** `manual` | `auto` */
	lifecycle_mode: string;
	/** id of the assigned flow, or null if the library uses none. */
	flow_id: number | null;
	/** Display name of the assigned flow (join-derived), or null. */
	flow_name: string | null;
	auto_queue: boolean;
	retention_days: number;
	auto_delete: boolean;
	scan_schedule: string | null;
	watchable: boolean;
}

export interface FileRow {
	id: number;
	library_id: number;
	path: string;
	dev: number;
	inode: number;
	size: number;
	mtime: number;
	sample_hash: number | null;
	/** Cached `FileFacts` JSON. */
	facts_json: string | null;
	/**
	 * `unscanned` | `scanning` | `compliant` | `queued` | `running` |
	 * `verifying` | `completed` | `failed` | `quarantined` | `unmatched`
	 */
	status: string;
	last_probed: number | null;
	last_evaluated: number | null;
	input_size: number | null;
	output_size: number | null;
}

export interface JobRow {
	id: number;
	file_id: number;
	library_id: number;
	flow_version: number;
	/** The `FfmpegPlan` JSON this job executes. */
	plan_json: string;
	/** `queued` | `running` | `verifying` | `completed` | `failed` | `quarantined` | `canceled` */
	state: string;
	device_id: string | null;
	claimed_by: string | null;
	lease_expires: number | null;
	started: number | null;
	ended: number | null;
	exit_kind: string | null;
	log_path: string | null;
	quarantine_path: string | null;
}

/** One frame on the /api/events stream (tagged by `type`). */
export interface ServerEvent {
	type:
		| "job_changed"
		| "job_log"
		| "file_changed"
		| "library_changed"
		| "flow_changed"
		| "tick"
		| "resync";
	job_id?: number;
	file_id?: number;
	library_id?: number;
	flow_id?: number;
	chunk?: string;
}

export interface Health {
	status: string;
	flow_version: number;
	/** Server version (package version). */
	version?: string;
	/** Build date (YYYY-MM-DD) stamped into the binary at compile time. */
	build?: string;
}

// ── Flow schema (GET /api/schema/flow) ─────────────────────────────

export interface SchemaField {
	description: string;
	schema: UiSchema;
}

/** A schema option: the wire value plus the display name (the UI renders
 *  labels; the server parses values). */
export type SchemaOption = { value: string; label: string };

/** The ui_schema vocabulary emitted by the core registry (exhaustive — the
 *  discriminated `kind` drives FlowField; no catch-all arm, so narrowing works).
 *  Every kind may carry a `label` (the display name for the field or
 *  section); the UI falls back to title-casing the schema key. */
export type UiSchema =
	| { kind: "multi_select"; label?: string; values: SchemaOption[]; hint?: string }
	| { kind: "single_select"; label?: string; values: SchemaOption[]; hint?: string; default?: string }
	| { kind: "resolution_range"; label?: string; hint?: string }
	| { kind: "byte_range"; label?: string; unit?: string; hint?: string }
	| { kind: "text"; label?: string; hint?: string; default?: string }
	| { kind: "boolean"; label?: string; hint?: string; default?: boolean }
	| { kind: "bitrate_mode"; label?: string; values?: SchemaOption[]; hint?: string; default?: string }
	| { kind: "device_select"; label?: string; hint?: string; default?: string }
	| {
			kind: "audio_policy";
			label?: string;
			values?: SchemaOption[];
			reencode?: {
				codec: { kind: "single_select"; label?: string; default?: string; values: SchemaOption[] };
				sample_rate?: { kind: "text"; label?: string; hint?: string };
				channels?: { kind: "text"; label?: string; hint?: string };
			};
			hint?: string;
			default?: string;
	  }
	| { kind: "resolution"; label?: string; hint?: string }
	| { kind: "tonemap"; label?: string; values: SchemaOption[]; hint?: string }
	| { kind: "list"; label?: string; item: Record<string, unknown>; hint?: string }
	| {
			kind: "object";
			label?: string;
			hint?: string;
			fields: Record<
				string,
				{
					kind: string;
					order?: number;
					label?: string;
					values?: SchemaOption[];
					hint?: string;
					default?: unknown;
					item?: unknown;
				}
			>;
	  };

export interface FlowSchema {
	flow_version: number;
	condition_fields: Record<string, SchemaField>;
	operation_sections: Record<string, SchemaField & { order?: number }>;
	devices: Device[];
}

// ── Flow document (libraries.flow_json) ───────────────────────────

export interface FlowCondition {
	[key: string]: unknown;
}

export interface FlowOperation {
	[key: string]: unknown;
}

export interface FlowStep {
	/** Optional stable id, preserved verbatim from the stored JSON. The
	 *  current editor neither reads nor writes it — array order is the
	 *  UI order; nothing assigns one. */
	id?: string;
	/** Optional user-assigned display name; unnamed steps show as "Step N". */
	name?: string;
	condition: FlowCondition;
	operation: FlowOperation;
}

export type NoMatchPolicy = { escalate?: boolean };

export interface Flow {
	flow_version: number;
	name?: string;
	steps: FlowStep[];
	no_match?: NoMatchPolicy;
}

// ── flows table row (first-class, library-independent) ───────────

export interface FlowRecord {
	id: number;
	name: string;
	/** The flow JSON as a string — parse for the Flow document. */
	flow_json: string;
	created_at: number;
	updated_at: number;
	/** Number of libraries currently referencing this flow. */
	library_count: number;
}
