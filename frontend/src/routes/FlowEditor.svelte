<script lang="ts">
	/**
	 * Flow editor: a flow's steps (condition + operation), rendered entirely
	 * from the flow schema (core §5) so adding a condition field, operation
	 * section, or verification check is a Rust-only change.
	 */
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import { Button } from "$lib/components/ui/button";
	import { Badge } from "$lib/components/ui/badge";
	import { Switch } from "$lib/components/ui/switch";
	import StepCard from "$lib/components/flow/StepCard.svelte";
	import { api } from "$lib/api";
	import { store } from "$lib/store.svelte";
	import { navigate } from "$lib/router.svelte.js";
	import { formatUnixSeconds } from "$lib/format";
	import type { Device, FlowSchema, FlowStep, NoMatchPolicy } from "$lib/types.js";

	let { id }: { id: number } = $props();

	let schema = $state<FlowSchema | null>(null);
	let flowRow = $state<{ id: number; name: string; flow_json: string; updated_at: number } | null>(null);
	let flow = $state<{
		steps: FlowStep[];
		no_match?: NoMatchPolicy;
	} | null>(null);
	let flowName = $state("");
	let busy = $state(false);
	let loadError = $state<string | null>(null);

	const steps = $derived(flow?.steps ?? []);
	const noMatch = $derived<NoMatchPolicy | null>(flow?.no_match ?? null);
	let noMatchEscalate = $state(false);
	function setNoMatchEscalate(v: boolean) {
		if (!flow) return;
		flow.no_match = { escalate: v };
		noMatchEscalate = v;
	}

	// The flow version constant — the only value ever written.
	const FLOW_VERSION = 1;

	// A visible-but-unset filter (empty list / no bounds) is a no-op — prune
	// it before sending so saved flows stay minimal.
	function pruneCondition(c: Record<string, unknown>): Record<string, unknown> {
		const out: Record<string, unknown> = {};
		for (const [k, raw] of Object.entries(c)) {
			if (raw === null || typeof raw !== "object") continue;
			const v = raw as Record<string, unknown>;
			const list = v["in"] ?? v["any_of"];
			if (Array.isArray(list) && list.length === 0) continue;
			if ((k === "resolution" || k === "file_size") && !("min" in v) && !("max" in v)) continue;
			if (Object.keys(v).length === 0) continue;
			out[k] = v;
		}
		return out;
	}

	async function save() {
		if (!schema || !flow) return;
		const name = flowName.trim();
		if (!name) {
			toast.warning("Flow name is required");
			return;
		}
		busy = true;
		try {
			// The editor only mutates; the stored JSON string is the
			// source of truth for fields the UI does not edit.
			//
			// `no_match` is omitted entirely when unset: the core type is
			// a plain struct (serde(default)), which accepts an absent key
			// but rejects an explicit null.
			const doc: Record<string, unknown> = {
				flow_version: FLOW_VERSION,
				steps: steps.map((s) => ({
					...s,
					condition: pruneCondition(s.condition as Record<string, unknown>) as typeof s.condition,
				})),
			};
			if (noMatch) doc.no_match = noMatch;
			const json = JSON.stringify(doc, null, 2);
			await api.updateFlow(id, { name, flow_json: json });
			// Refresh the dirty-check baseline with what was just stored,
			// or the badge would stay lit against the pre-save snapshot.
			if (flowRow) {
				flowRow = { ...flowRow, name, flow_json: json, updated_at: Math.floor(Date.now() / 1000) };
			}
			await store.refreshCore();
			toast.success("Saved — libraries using this flow are re-evaluating");
		} catch (e) {
			toast.error(`Failed to save flow: ${e instanceof Error ? e.message : e}`);
		} finally {
			busy = false;
		}
	}

	function addStep() {
		if (!flow) return;
		flow.steps = [...flow.steps, { condition: {}, operation: {} }];
	}

	function removeStep(i: number) {
		if (!flow) return;
		flow.steps = flow.steps.filter((_, j) => j !== i);
	}

	function moveStep(i: number, dir: -1 | 1) {
		if (!flow) return;
		const j = i + dir;
		if (j < 0 || j >= flow.steps.length) return;
		const arr = [...flow.steps];
		[arr[i], arr[j]] = [arr[j], arr[i]];
		flow.steps = arr;
	}

	// Both sides of the dirty check are normalized the same way — no-op
	// filter rows pruned, absent keys coerced — so visible-but-inert UI
	// state never reads as "unsaved" against an equivalent stored document.
	const normStep = (s: FlowStep) => ({
		condition: pruneCondition((s.condition ?? {}) as Record<string, unknown>),
		operation: s.operation ?? {},
	});
	const unsaved = $derived(
		flow !== null &&
			JSON.stringify({
				steps: steps.map(normStep),
				no_match: noMatch ?? null,
				name: flowName,
			}) !==
				JSON.stringify({
					steps: (flowRow?.flow_json ? (JSON.parse(flowRow.flow_json).steps as FlowStep[]) : []).map(normStep),
					no_match:
						(flowRow?.flow_json ? (JSON.parse(flowRow.flow_json).no_match as NoMatchPolicy | undefined) : undefined) ?? null,
					name: flowRow?.name ?? "",
				}),
	);
	const dirty = $derived(unsaved || flowName !== (flowRow?.name ?? ""));

	onMount(async () => {
		try {
			const [sc, record] = await Promise.all([api.schemaFlow(), id ? api.getFlow(id) : Promise.resolve(null)]);
			schema = sc;
			if (record) {
				flowName = record.name;
				const parsed = JSON.parse(record.flow_json) as {
					steps: FlowStep[];
					no_match?: NoMatchPolicy;
				};
				flow = parsed;
				noMatchEscalate = parsed.no_match?.escalate ?? false;
				flowRow = {
					id: record.id,
					name: record.name,
					flow_json: record.flow_json,
					updated_at: record.updated_at,
				};
			}
		} catch (e) {
			loadError = e instanceof Error ? e.message : String(e);
		}
	});

	const usingCount = $derived(store.flows.find((f) => f.id === id)?.library_count ?? 0);
</script>

{#if id}
	{#if loadError}
		<div class="py-12 text-center text-sm text-destructive">Couldn't load this flow: {loadError}</div>
	{:else if !flow}
		<div class="py-12 text-center text-sm text-muted-foreground">Loading…</div>
		{:else}
		<div class="flex flex-col gap-4">
			<div class="flex flex-wrap items-center gap-2">
				<Button variant="ghost" size="sm" onclick={() => navigate("/flows")}>
					← Flows
				</Button>
				<input
					class="h-9 w-64 rounded-md border border-input bg-transparent px-2 text-sm"
					value={flowName}
					placeholder="Flow name"
					oninput={(e) => (flowName = (e.target as HTMLInputElement).value)}
				/>
				{#if dirty}<Badge class="bg-amber-500/20 text-amber-600 dark:text-amber-400">Unsaved changes</Badge>{/if}
				{#if usingCount > 0}
					<Badge variant="secondary" title="Libraries that use this flow">{usingCount} {usingCount === 1 ? "library" : "libraries"}</Badge>
				{/if}
				<Button class="ml-auto" disabled={!dirty || busy} onclick={save}>{busy ? "Saving…" : "Save"}</Button>
			</div>
			<p class="text-xs text-muted-foreground">
				Re-encodes files that match its steps. Leave a section off and that part of the file is untouched.
			</p>

			<div class="flex flex-col gap-3">
				{#each steps as step, i (i)}
					<StepCard
						step={step}
						index={i}
						total={steps.length}
						schema={schema!}
						devices={store.devices as Device[]}
						onRemove={() => removeStep(i)}
						onMoveUp={() => moveStep(i, -1)}
						onMoveDown={() => moveStep(i, 1)}
					/>
				{:else}
					<div class="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
						No steps yet — a file must match a step's filters to be transcoded.
					</div>
				{/each}
				<Button variant="outline" size="sm" class="self-start" onclick={addStep}>
					+ Add step
				</Button>
			</div>

			<div class="rounded-lg border p-3">
				<div class="flex items-center gap-2">
					<Switch checked={noMatchEscalate} onCheckedChange={setNoMatchEscalate} />
					<span class="text-sm font-medium">Warn on unmatched files</span>
				</div>
				<p class="mt-1 text-xs text-muted-foreground">
					Files that match no step are left as they are; this flag surfaces them as warnings instead.
				</p>
			</div>

			{#if flowRow}
				<p class="text-xs text-muted-foreground">Last saved {formatUnixSeconds(flowRow.updated_at)}</p>
			{/if}
		</div>
	{/if}
{:else}
	<div class="py-12 text-center text-sm text-muted-foreground">No flow selected.</div>
{/if}
