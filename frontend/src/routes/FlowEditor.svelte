<script lang="ts">
	/**
	 * Schema-driven flow editor (design §5, §9.3): every condition field and
	 * operation section is rendered from GET /api/schema/flow — nothing is
	 * hardcoded. Impact preview is a v1 gap (no evaluate endpoint yet).
	 */
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import * as Card from "$lib/components/ui/card";
	import * as Tabs from "$lib/components/ui/tabs";
	import * as Empty from "$lib/components/ui/empty";
	import { Badge } from "$lib/components/ui/badge";
	import { Button } from "$lib/components/ui/button";
	import SyncSwitch from "$lib/components/flow/SyncSwitch.svelte";
	import { Textarea } from "$lib/components/ui/textarea";
	import { Label } from "$lib/components/ui/label";
	import StepCard from "$lib/components/flow/StepCard.svelte";
	import { api } from "$lib/api.js";
	import { navigate } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";
	import type { Flow, FlowSchema } from "$lib/types.js";

	let { id }: { id: number } = $props();

	const lib = $derived(store.libraries.find((l) => l.id === id));

	let schema = $state<FlowSchema | null>(null);
	let flow = $state<Flow | null>(null);
	let savedJson = $state("");
	let loadingError = $state<string | null>(null);

	let tab = $state<"steps" | "json">("steps");
	let jsonText = $state("");

	onMount(async () => {
		try {
			schema = await api.schemaFlow();
			const l = await api.getLibrary(id);
			const parsed = JSON.parse(l.flow_json) as Flow;
			if (schema && parsed.flow_version !== schema.flow_version) {
				throw new Error(
					`Flow version mismatch: library has v${parsed.flow_version}, server speaks v${schema.flow_version}`,
				);
			}
			flow = parsed;
			savedJson = JSON.stringify(parsed, null, 2);
			jsonText = savedJson;
		} catch (e) {
			loadingError = e instanceof Error ? e.message : String(e);
		}
	});

	const dirty = $derived(
		flow != null && JSON.stringify(flow, null, 2) !== savedJson,
	);
	const jsonDirty = $derived(flow != null && jsonText !== JSON.stringify(flow, null, 2)); // unapplied JSON-tab edits must be Applied before save
	// Keep the Raw JSON tab live: re-serialize when the step editor mutates flow.
	$effect(() => {
		const f = flow;
		if (f) jsonText = JSON.stringify(f, null, 2);
	});

	function addStep() {
		if (!flow) return;
		flow.steps = [
			...flow.steps,
			{
				id: `s${Math.random().toString(36).slice(2, 10)}`,
				condition: {},
				operation: {},
			},
		];
	}
	function removeStep(i: number) {
		if (!flow) return;
		flow.steps = flow.steps.filter((_, j) => j !== i);
	}
	function moveStep(i: number, dir: -1 | 1) {
		if (!flow) return;
		const j = i + dir;
		if (j < 0 || j >= flow.steps.length) return;
		const steps = [...flow.steps];
		[steps[i], steps[j]] = [steps[j], steps[i]];
		flow.steps = steps;
	}

	function syncJsonFromSteps() {
		if (flow) jsonText = JSON.stringify(flow, null, 2);
	}
	function applyJson() {
		try {
			const parsed = JSON.parse(jsonText) as Flow;
			if (schema && parsed.flow_version !== schema.flow_version) {
				throw new Error(`flow_version must be ${schema.flow_version}`);
			}
			if (!Array.isArray(parsed.steps)) throw new Error("missing steps array");
			flow = parsed;
			tab = "steps";
			toast.success("Applied JSON to the step editor");
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
		}
	}

	let saving = $state(false);
	async function save() {
		if (!flow || !lib) return;
		if (jsonDirty) {
			toast.error("Apply the Raw JSON changes first, then save");
			return;
		}
		saving = true;
		try {
			const flowJson = JSON.stringify(flow);
			await api.updateLibrary({ ...lib, flow_json: flowJson });
			savedJson = JSON.stringify(flow, null, 2);
			await store.refreshCore();
			toast.success("Flow saved — the library is being re-scanned with the new rules");
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
		} finally {
			saving = false;
		}
	}

	function setEscalate(v: boolean) {
		if (!flow) return;
		flow.no_match = { escalate: v };
	}
	const escalate = $derived(flow?.no_match?.escalate ?? false);
</script>

{#if loadingError}
	<Empty.Root class="my-16 flex-col">
		<Empty.Title>Could not load flow</Empty.Title>
		<Empty.Content>{loadingError}</Empty.Content>
	</Empty.Root>
{:else if !schema || !flow || !lib}
	<Empty.Root class="my-16 flex-col">
		<Empty.Title>Loading schema…</Empty.Title>
	</Empty.Root>
{:else}
	<div class="flex items-start justify-between gap-4">
		<div>
			<div class="flex items-center gap-3">
				<h1 class="text-2xl font-semibold tracking-tight">Flow — {lib.name}</h1>
				<Badge variant="outline">v{schema.flow_version}</Badge>
				{#if dirty}
					<Badge variant="destructive">unsaved</Badge>
				{/if}
			</div>
			<p class="text-muted-foreground">
				Steps are evaluated top to bottom; the first match wins. Files matching no step are marked
				unmatched{escalate ? " and escalate to warnings" : ""}.
			</p>
		</div>
		<div class="flex shrink-0 gap-2">
			<Button variant="outline" onclick={addStep}>
				<PlusIcon class="size-4" data-icon="inline-start" />
				Add step
			</Button>
			<Button onclick={save} disabled={!dirty || saving || jsonDirty}>
				{saving ? "Saving…" : "Save flow"}
			</Button>
		</div>
	</div>

	<div class="mt-4 flex items-center gap-3 rounded-lg border p-3">
		<SyncSwitch enabled={escalate} onSet={setEscalate} />
		<div>
			<p class="text-sm font-medium">Warn on no-match</p>
			<p class="text-xs text-muted-foreground">
				Escalate files that match no step to warnings instead of silently marking them unmatched.
			</p>
		</div>
	</div>

	<div class="mt-4">
		<Tabs.Root bind:value={tab}>
			<Tabs.List>
				<Tabs.Trigger value="steps">Steps ({flow.steps.length})</Tabs.Trigger>
				<Tabs.Trigger value="json">Raw JSON</Tabs.Trigger>
			</Tabs.List>
			<Tabs.Content value="steps" class="pt-4">
				{#if flow.steps.length === 0}
					<Empty.Root class="border-dashed">
						<Empty.Title>No steps</Empty.Title>
						<Empty.Content>
							With no steps, every file is evaluated as unmatched. Add a step to start transcoding.
						</Empty.Content>
						<div class="mt-2">
							<Button onclick={addStep}>
								<PlusIcon class="size-4" data-icon="inline-start" />
								Add the first step
							</Button>
						</div>
					</Empty.Root>
				{:else}
					<div class="flex flex-col gap-3">
						{#each flow.steps as step, i (step.id)}
							<StepCard
								{step}
								index={i}
								total={flow.steps.length}
								{schema}
								devices={store.devices}
								onRemove={() => removeStep(i)}
								onMoveUp={() => moveStep(i, -1)}
								onMoveDown={() => moveStep(i, 1)}
							/>
						{/each}
						<Button variant="outline" onclick={addStep}>
							<PlusIcon class="size-4" data-icon="inline-start" />
							Add step
						</Button>
					</div>
				{/if}
			</Tabs.Content>
			<Tabs.Content value="json" class="pt-4">
				<div class="flex flex-col gap-2">
					<Label for="flow-json">Flow JSON (validated against v{schema.flow_version})</Label>
					<Textarea id="flow-json" bind:value={jsonText} class="min-h-96 font-mono text-xs" spellcheck={false} />
					<div class="flex gap-2">
						<Button
							variant="ghost"
							onclick={() => {
								syncJsonFromSteps();
								toast.success("JSON refreshed from the step editor");
							}}
						>
							Refresh from steps
						</Button>
						<Button variant="outline" onclick={applyJson} disabled={saving || !jsonDirty}>
							{saving ? "Applying…" : "Apply"}
						</Button>
					</div>
				</div>
			</Tabs.Content>
		</Tabs.Root>
	</div>
{/if}
