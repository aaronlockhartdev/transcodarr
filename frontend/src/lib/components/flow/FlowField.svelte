<script lang="ts">
	/**
	 * Generic renderer for one ui_schema field (design §5: the UI is driven
	 * entirely by GET /api/schema/flow — no hardcoded field lists).
	 *
	 * Single-value pickers use a native <select> styled to the design system:
	 * bits-ui's Select types `value` as a single/multiple union which doesn't
	 * fit string state, and a native control keeps this component simple.
	 */
	import { Switch } from "$lib/components/ui/switch";
	import { Input } from "$lib/components/ui/input";
	import { Button } from "$lib/components/ui/button";
	import Undo2Icon from "@lucide/svelte/icons/undo-2";
	import { cn } from "$lib/utils.js";
	import type { Device, UiSchema } from "$lib/types.js";

	const SELECT_CLASS =
		"h-8 w-full rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50";

	let {
		schema,
		value = $bindable(),
		devices = $bindable([] as Device[]),
		class: className,
	}: {
		schema: UiSchema;
		value?: unknown;
		devices?: Device[];
		class?: string;
	} = $props();

	// --- local mirrors (controls need strings/booleans; props are union-typed)
	let textValue = $state("");
	$effect(() => {
		textValue = typeof value === "string" ? value : value == null ? "" : String(value);
	});
	let boolValue = $state(false);
	$effect(() => {
		boolValue = value === true;
	});

	function setText(v: string) {
		value = v === "" ? undefined : v;
	}
	function setBool(v: boolean) {
		value = v;
	}

	// single_select
	const sel = $derived(typeof value === "string" ? value : "");
	function selectChanged(v: string) {
		value = v;
	}

	// multi_select (string[] value)
	const listValue = $derived(Array.isArray(value) ? (value as string[]) : []);
	function toggle(v: string) {
		const list = Array.isArray(value) ? [...(value as string[])] : [];
		const i = list.indexOf(v);
		if (i === -1) list.push(v);
		else list.splice(i, 1);
		value = list;
	}

	// bitrate_mode: {mode, bps?, value?}
	const bitrate = $derived.by(() => {
		const b = value as { mode?: string; bps?: number; value?: number } | undefined;
		return {
			mode: b?.mode ?? "source_capped",
			bps: b?.bps ?? 0,
			crf: b?.value ?? 23,
		};
	});
	function setBitrateMode(mode: string) {
		if (mode === "source_capped") value = { mode };
		else if (mode === "fixed") value = { mode, bps: bitrate.bps || 6_000_000 };
		else value = { mode, value: bitrate.crf };
	}
	function setBpsMega(mbps: string) {
		const bps = Math.round(Number(mbps) * 1_000_000);
		value = { mode: "fixed", bps: bps > 0 ? bps : 6_000_000 };
	}
	function setCrf(v: string) {
		const n = Number(v);
		value = { mode: "crf", value: n > 0 ? n : 23 };
	}

	// device_select: "auto" | "software" | {kind:'gpu', id}
	const deviceSel = $derived.by(() => {
		if (typeof value === "string") return value;
		if (value && typeof value === "object" && (value as { kind?: string }).kind === "gpu")
			return `gpu:${(value as { id?: string }).id}`;
		return "auto";
	});
	function deviceChanged(v: string) {
		if (v.startsWith("gpu:")) value = { kind: "gpu", id: v.slice(4) };
		else value = v;
	}
	const deviceOptions = $derived([
		{ value: "auto", label: "Auto" },
		{ value: "software", label: "CPU (software)" },
		...devices.filter((d) => d.kind === "gpu").map((d) => ({ value: `gpu:${d.id}`, label: d.name })),
	]);

	// audio_policy: "copy" | "drop" | {codec, sample_rate?, channels?}
	// Empty rate/channels mean "keep the source" — the key is omitted
	// entirely (same semantics as the rule-action editor).
	const policy = $derived.by(() => {
		if (typeof value === "string")
			return {
				kind: value as string,
				codec: "",
				sampleRate: null as number | null,
				channels: null as number | null,
			};
		const p = value as { codec?: string; sample_rate?: number; channels?: number } | undefined;
		return {
			kind: "re_encode",
			codec: p?.codec ?? "",
			sampleRate: p?.sample_rate ?? null,
			channels: p?.channels ?? null,
		};
	});
	const policySel = $derived(policy.kind);
	// Option vocabularies come from the server schema, never hardcoded.
	type Opt = { value: string; label: string };
	const schemaOpts = $derived((schema as { values?: Opt[] }).values ?? []);
	const reencodeCodecs = $derived(
		((schema as { reencode?: { codec?: { values?: Opt[] } } }).reencode?.codec?.values ?? []),
	);
	const codecSel = $derived(policy.codec || reencodeCodecs[0]?.value || "eac3");
	function policyChanged(kind: string) {
		if (kind === "copy") value = "copy";
		else if (kind === "drop") value = "drop";
		else {
			const a: Record<string, unknown> = { codec: policy.codec || reencodeCodecs[0]?.value || "eac3" };
			if (policy.sampleRate != null) a.sample_rate = policy.sampleRate;
			if (policy.channels != null) a.channels = policy.channels;
			value = a;
		}
	}
	function codecChanged(codec: string) {
		// Changing the codec must not silently pin rate/channels.
		const a: Record<string, unknown> = { codec };
		if (policy.sampleRate != null) a.sample_rate = policy.sampleRate;
		if (policy.channels != null) a.channels = policy.channels;
		value = a;
	}
	function setPolicyRate(v: string) {
		const n = Number(v);
		const a: Record<string, unknown> = { codec: codecSel };
		if (policy.channels != null) a.channels = policy.channels;
		if (Number.isFinite(n) && n > 0) a.sample_rate = Math.round(n);
		value = a;
	}
	function setPolicyChannels(v: string) {
		const n = Number(v);
		const a: Record<string, unknown> = { codec: codecSel };
		if (policy.sampleRate != null) a.sample_rate = policy.sampleRate;
		if (Number.isFinite(n) && n > 0) a.channels = Math.round(n);
		value = a;
	}

	// resolution (op field): [w, h] | undefined
	const res = $derived.by(() => {
		if (Array.isArray(value) && value.length === 2) {
			return { w: value[0] as number, h: value[1] as number };
		}
		return { w: 0, h: 0 };
	});
	function setRes(w: string, h: string) {
		const ww = Number(w);
		const hh = Number(h);
		value = ww > 0 && hh > 0 ? [ww, hh] : undefined;
	}
</script>

{#if schema.kind === "single_select"}
	<select
		class={cn(SELECT_CLASS, className)}
		value={sel}
		onchange={(e) => selectChanged((e.target as HTMLSelectElement).value)}
	>
		{#each schema.values as v (v.value)}
			<option value={v.value}>{v.label}</option>
		{/each}
	</select>
{:else if schema.kind === "text"}
	<Input bind:value={textValue} oninput={() => setText(textValue)} class={className} placeholder={schema.hint ?? "auto"} />
{:else if schema.kind === "boolean"}
	<label class="flex items-center gap-2">
		<Switch bind:checked={boolValue} onchange={() => setBool(!boolValue)} />
		<span class="text-sm">{schema.label ?? schema.hint ?? "Toggle"}</span>
	</label>
{:else if schema.kind === "multi_select"}
	<div class="flex flex-wrap gap-1.5">
		{#each schema.values as v (v.value)}
			<Button
				type="button"
				variant={listValue.includes(v.value) ? "default" : "outline"}
				size="sm"
				class="h-auto px-2 py-1 text-xs"
				onclick={() => toggle(v.value)}
			>
				{v.label}
			</Button>
		{/each}
	</div>
{:else if schema.kind === "bitrate_mode"}
	<div class="flex flex-col gap-2">
		<div class="flex gap-1">
			{#each schemaOpts as v (v.value)}
				<Button
					type="button"
					variant={bitrate.mode === v.value ? "default" : "outline"}
					size="sm"
					class="h-auto px-2 py-1 text-xs capitalize"
					onclick={() => setBitrateMode(v.value)}
				>
					{v.label}
				</Button>
			{/each}
		</div>
		{#if bitrate.mode === "fixed"}
			<div class="flex items-center gap-2">
				<Input type="number" min="1" value={String(Math.round(bitrate.bps / 1_000_000))} oninput={(e) => setBpsMega((e.target as HTMLInputElement).value)} class="w-28" />
				<span class="text-sm text-muted-foreground">Mbps</span>
			</div>
		{:else if bitrate.mode === "crf"}
			<div class="flex items-center gap-2">
				<Input type="number" min="0" max="51" value={String(bitrate.crf)} oninput={(e) => setCrf((e.target as HTMLInputElement).value)} class="w-28" />
				<span class="text-sm text-muted-foreground">CRF</span>
			</div>
		{:else}
			<p class="text-xs text-muted-foreground">{schema.hint}</p>
		{/if}
	</div>
{:else if schema.kind === "device_select"}
	<select
		class={cn(SELECT_CLASS, className)}
		value={deviceSel}
		onchange={(e) => deviceChanged((e.target as HTMLSelectElement).value)}
	>
		{#each deviceOptions as v (v.value)}
			<option value={v.value}>{v.label}</option>
		{/each}
	</select>
{:else if schema.kind === "audio_policy"}
	<div class="flex flex-col gap-2">
		<select
			class={cn(SELECT_CLASS, className)}
			value={policySel}
			onchange={(e) => policyChanged((e.target as HTMLSelectElement).value)}
		>
			{#each schemaOpts as v (v.value)}
				<option value={v.value}>{v.label}</option>
			{/each}
		</select>
		{#if policy.kind === "re_encode"}
			<div class="flex flex-wrap items-center gap-2">
				<select
					class={cn(SELECT_CLASS, "w-32")}
					value={codecSel}
					onchange={(e) => codecChanged((e.target as HTMLSelectElement).value)}
				>
					{#each reencodeCodecs as v (v.value)}
						<option value={v.value}>{v.label}</option>
					{/each}
				</select>
				<Input type="number" value={policy.sampleRate != null ? String(policy.sampleRate) : ""} title="Empty keeps the source rate" oninput={(e) => setPolicyRate((e.target as HTMLInputElement).value)} class="w-24" placeholder="rate" />
				<span class="text-xs text-muted-foreground">Hz</span>
				<Input type="number" min="1" value={policy.channels != null ? String(policy.channels) : ""} title="Empty keeps the source channel count" oninput={(e) => setPolicyChannels((e.target as HTMLInputElement).value)} class="w-16" placeholder="ch" />
				<span class="text-xs text-muted-foreground">ch</span>
				<Button variant="ghost" size="sm" title="Reset this policy to Copy" onclick={() => policyChanged("copy")}>
					<Undo2Icon class="size-3.5" />
				</Button>
			</div>
		{/if}
	</div>
{:else if schema.kind === "resolution"}
	<div class="flex items-center gap-2">
		<Input type="number" min="0" value={res.w ? String(res.w) : ""} oninput={(e) => setRes((e.target as HTMLInputElement).value, String(res.h))} class="w-24" placeholder="width" />
		<span class="text-muted-foreground">×</span>
		<Input type="number" min="0" value={res.h ? String(res.h) : ""} oninput={(e) => setRes(String(res.w), (e.target as HTMLInputElement).value)} class="w-24" placeholder="height" />
		<span class="text-xs text-muted-foreground">{schema.hint ?? ""}</span>
	</div>
{:else}
	<p class="text-xs text-muted-foreground">unsupported field kind: {schema.kind}</p>
{/if}
