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

/** The ui_schema vocabulary emitted by the core registry (exhaustive — the
 *  discriminated `kind` drives FlowField; no catch-all arm, so narrowing works). */
export type UiSchema =
	| { kind: "multi_select"; values: string[]; hint?: string }
	| { kind: "single_select"; values: { value: string; label: string }[]; hint?: string; default?: string }
	| { kind: "resolution_range"; common: Record<string, [number, number]>; hint?: string }
	| { kind: "byte_range"; unit?: string; hint?: string }
	| { kind: "text"; hint?: string; default?: string }
	| { kind: "boolean"; hint?: string; default?: boolean }
	| { kind: "bitrate_mode"; values?: { value: string; label: string }[]; hint?: string; default?: string }
	| { kind: "device_select"; hint?: string; default?: string }
	| { kind: "audio_policy"; values?: { value: string; label: string }[]; reencode?: { codec: { kind: "single_select"; default?: string; values: { value: string; label: string }[] }; sample_rate?: { kind: "text"; hint?: string }; channels?: { kind: "text"; hint?: string } }; hint?: string; default?: string }
	| { kind: "resolution"; hint?: string }
	| { kind: "list"; item: Record<string, unknown>; hint?: string }
	| {
			kind: "object";
			hint?: string;
			fields: Record<
				string,
				{ kind: string; values?: string[] | { value: string; label: string }[]; hint?: string; default?: unknown; item?: unknown; common?: Record<string, [number, number]> }
			>;
	  };

export interface FlowSchema {
	flow_version: number;
	condition_fields: Record<string, SchemaField>;
	operation_sections: Record<string, SchemaField>;
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
	id: string;
	condition: FlowCondition;
	operation: FlowOperation;
}

export interface Flow {
	flow_version: number;
	name?: string;
	steps: FlowStep[];
	no_match?: { escalate?: boolean };
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
