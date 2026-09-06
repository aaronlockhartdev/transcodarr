<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import * as Card from "$lib/components/ui/card";
	import { Badge } from "$lib/components/ui/badge";
	import { Button } from "$lib/components/ui/button";
	import StepCard from "$lib/components/flow/StepCard.svelte";
	import { api, ApiError } from "$lib/api.js";
	import { navigate } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";
	import type { Flow, FlowSchema } from "$lib/types.js";

	let { id }: { id: number } = $props();

	// The flows row behind this editor (live from the store — library count, name).
	const flowRow = $derived(store.flows.find((f) => f.id === id) ?? null);

	let schema = $state<FlowSchema | null>(null);
	let flow = $state<Flow | null>(null);
	let loaded = $state(false);
	let loadError = $state<string | null>(null);
	let unsaved = $state(false);
	let saving = $state(false);
	let flowName = $state("");

	// The header name is live-editable; a name change counts as an edit too.
	const dirty = $derived(unsaved || flowName !== (flowRow?.name ?? ""));

	onMount(async () => {
		try {
			const [schemaRes, record] = await Promise.all([api.schemaFlow(), api.getFlow(id)]);
			schema = schemaRes;
			flow = JSON.parse(record.flow_json) as Flow;
			flowName = record.name;
			loaded = true;
		} catch (e) {
			loadError = e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e);
		}
	});

	// Every mutation marks the form dirty; save() is the only write path.
	function touch<T>(fn: (f: Flow) => T): T {
		unsaved = true;
		return fn(flow!);
	}

	function addStep() {
		touch((f) => {
			f.steps = f.steps ?? [];
			f.steps.push({
				id: `step_${Date.now()}_${f.steps.length}`,
				condition: {},
				operation: {},
			});
		});
	}

	function removeStep(idToRemove: string) {
		touch((f) => {
			f.steps = (f.steps ?? []).filter((s) => s.id !== idToRemove);
		});
	}

	function moveStep(idToMove: string, dir: -1 | 1) {
		touch((f) => {
			const steps = f.steps ?? [];
			const i = steps.findIndex((s) => s.id === idToMove);
			const j = i + dir;
			if (i < 0 || j < 0 || j >= steps.length) return;
			const [s] = steps.splice(i, 1);
			steps.splice(j, 0, s);
		});
	}

	function setNoMatchEscalate(escalate: boolean) {
		touch((f) => {
			f.no_match = { escalate };
		});
	}

	async function save() {
		if (!flow || !flowRow) return;
		const name = flowName.trim();
		if (name === "") {
			toast.error("Flow name is required");
			return;
		}
		saving = true;
		try {
			const json = JSON.stringify(flow);
			await api.updateFlow(id, { name, flow_json: json });
			// Re-parse what the server stored (it validates and re-evaluates the
			// using libraries on save) and refresh the store so the sidebar
			// library counts pick up the new evaluations.
			flow = JSON.parse(json) as Flow;
			unsaved = false;
			await store.refreshCore();
			toast.success("Saved — libraries using this flow are re-evaluating");
		} catch (e) {
			toast.error(e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));
		} finally {
			saving = false;
		}
	}
</script>

{#if !loaded}
	{#if loadError}
		<div class="py-16 text-center text-sm text-destructive">
			{loadError}
		</div>
	{:else}
		<div class="py-16 text-center text-sm text-muted-foreground">Loading flow…</div>
	{/if}
{:else if schema && flow}
	<button
		class="mb-2 flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
		onclick={() => navigate("/flows")}
	>
		<ChevronLeftIcon class="size-4" data-icon="inline-start" />
		Flows
	</button>

	<div class="flex flex-wrap items-center gap-3">
		<input
			class="h-9 w-56 rounded-md border border-input bg-transparent px-3 text-xl font-semibold tracking-tight outline-none focus-visible:ring-1 focus-visible:ring-ring"
			placeholder="Flow name"
			bind:value={flowName}
		/>
		<Badge variant="secondary" class="font-mono">v{flow.flow_version}</Badge>
		{#if flowRow && flowRow.library_count > 0}
			<Badge variant="secondary">{flowRow.library_count} libraries</Badge>
		{/if}
		{#if dirty}
			<Badge variant="outline" class="border-amber-500 text-amber-600 dark:border-amber-400 dark:text-amber-300">
				unsaved changes
			</Badge>
		{/if}
		<span class="flex-1"></span>
		<Button onclick={save} disabled={saving || !dirty}>
			{saving ? "Saving…" : "Save"}
		</Button>
	</div>
	<p class="text-sm text-muted-foreground">
		Steps are evaluated top to bottom; the first match decides. Saving re-evaluates every library that
		uses this flow.
	</p>

	<Card.Root class="mt-4">
		<Card.Header>
			<Card.Title>Steps</Card.Title>
			<Card.Description>Order matters — first matching step wins.</Card.Description>
		</Card.Header>
		<Card.Content>
			{#each flow.steps ?? [] as step, i (step.id)}
				<StepCard
					{step}
					index={i}
					total={(flow.steps ?? []).length}
					{schema}
					devices={schema.devices}
					onRemove={() => removeStep(step.id)}
					onMoveUp={() => moveStep(step.id, -1)}
					onMoveDown={() => moveStep(step.id, 1)}
				/>
			{:else}
				<div class="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
					No steps yet — this flow matches nothing.
				</div>
			{/each}

			<div class="mt-4 flex items-center justify-between gap-4 border-t pt-4">
				<div class="flex flex-col gap-0.5">
					<span class="text-sm font-medium">No matching step</span>
					<span class="text-sm text-muted-foreground">Queue a failed job so the file is visible instead of silently compliant</span>
				</div>
				<Button
					variant={flow.no_match?.escalate ? "default" : "outline"}
					size="sm"
					onclick={() => (flow ? setNoMatchEscalate(!(flow.no_match?.escalate)) : undefined)}
				>
					{flow.no_match?.escalate ? "Escalating" : "Not escalating"}
				</Button>
			</div>

			<Button class="mt-4" variant="outline" onclick={addStep}>
				Add step
			</Button>
		</Card.Content>
	</Card.Root>
{:else}
	<div class="py-16 text-center text-sm text-muted-foreground">Flow schema unavailable.</div>
{/if}
