<script lang="ts">
	/**
	 * One flow step: condition (all fields from the schema) + operation
	 * sections (video / audio / subtitles / container). Absent section =
	 * identity; present section = the editor's values.
	 */
	import { Button } from "$lib/components/ui/button";
	import { Input } from "$lib/components/ui/input";
	import SyncSwitch from "./SyncSwitch.svelte";
	import ChevronUpIcon from "@lucide/svelte/icons/chevron-up";
	import ChevronDownIcon from "@lucide/svelte/icons/chevron-down";
	import TrashIcon from "@lucide/svelte/icons/trash-2";
	import FlowField from "./FlowField.svelte";
	import type { Device, FlowStep, FlowSchema, UiSchema } from "$lib/types.js";

	let {
		step,
		index,
		total,
		schema,
		devices,
		onRemove,
		onMoveUp,
		onMoveDown,
	}: {
		step: FlowStep;
		index: number;
		total: number;
		schema: FlowSchema;
		devices: Device[];
		onRemove: () => void;
		onMoveUp: () => void;
		onMoveDown: () => void;
	} = $props();

	// ---- condition helpers -------------------------------------------------
	// Constraint key per multi_select field (serialization contract, core §5):
	// audio_codec matches ANY track, all other fields are exact-membership.
	function constraintKey(field: string): "in" | "any_of" {
		return field === "audio_codec" ? "any_of" : "in";
	}
	function listFor(field: string): string[] {
		const c = step.condition[field] as Record<string, unknown> | undefined;
		if (!c) return [];
		const key = constraintKey(field);
		return Array.isArray(c[key]) ? (c[key] as string[]) : [];
	}
	function setList(field: string, list: string[]) {
		const key = constraintKey(field);
		const next = { ...step.condition };
		if (list.length === 0) delete next[field];
		else next[field] = { [key]: list };
		step.condition = next;
	}
	function toggleChip(field: string, v: string) {
		const list = [...listFor(field)];
		const i = list.indexOf(v);
		if (i === -1) list.push(v);
		else list.splice(i, 1);
		setList(field, list);
	}

	// resolution_range: {min?: [w,h], max?: [w,h]}
	type ResRange = { min?: [number, number]; max?: [number, number] };
	function resFor(): ResRange {
		return (step.condition["resolution"] as ResRange) ?? {};
	}
	function setRes(which: "min" | "max", w: number, h: number) {
		const r: ResRange = { ...resFor() };
		if (w > 0 && h > 0) r[which] = [w, h];
		else delete r[which];
		if (!r.min && !r.max) delete step.condition["resolution"];
		else step.condition = { ...step.condition, resolution: r };
	}

	// byte_range: {min?, max?} in bytes
	type ByteRange = { min?: number; max?: number };
	function sizeFor(): ByteRange {
		return (step.condition["file_size"] as ByteRange) ?? {};
	}
	function setSize(which: "min" | "max", mb: string) {
		const s: ByteRange = { ...sizeFor() };
		const bytes = Math.round(Number(mb) * 1_000_000);
		if (mb && bytes > 0) s[which] = bytes;
		else delete s[which];
		if (!s.min && !s.max) delete step.condition["file_size"];
		else step.condition = { ...step.condition, file_size: s };
	}

	// ---- operation helpers --------------------------------------------------
	// NOTE: `name in step.operation` is NOT a tracked read on the $state proxy
	// (only property gets subscribe), so use an explicit get here.
	function sectionEnabled(name: string): boolean {
		return (step.operation as Record<string, unknown>)[name] !== undefined;
	}
	function setSection(name: string, enabled: boolean) {
		const next = { ...step.operation };
		if (enabled) {
			if (next[name] === undefined) {
				// seedSection wraps the schema field defaults in wire shape.
				next[name] = seedSection(name);
			}
		} else {
			delete next[name];
		}
		step.operation = next;
	}
	// Binding-target map into the live (reactive) operation object.
	const op = $derived.by(() => {
		const o = step.operation as unknown as Record<string, any>;
		return o;
	});

	function sectionValue(name: string): Record<string, unknown> {
		const v = step.operation[name];
		return (v as Record<string, unknown>) ?? {};
	}
	// Enable-time defaults, from the schema (never hardcoded): each field's
	// default wrapped in its wire shape, so an enabled section is complete
	// and parseable (video, for instance, requires codec).
	function seedSection(name: string): Record<string, unknown> {
		const f = schema.operation_sections[name]?.schema as
			| { fields?: Record<string, { kind?: string; default?: unknown }> }
			| undefined;
		const seeded: Record<string, unknown> = {};
		for (const [k, fld] of Object.entries(f?.fields ?? {})) {
			if (fld.default === undefined) continue;
			seeded[k] = fld.kind === "bitrate_mode" ? { mode: fld.default } : fld.default;
		}
		return seeded;
	}
	function setSectionField(name: string, field: string, value: unknown) {
		const sec = { ...sectionValue(name) };
		if (value === undefined) delete sec[field];
		else sec[field] = value;
		step.operation = { ...step.operation, [name]: sec };
	}

	// audio rules
	type Rule = { match: { codecs: string[]; languages: string[] }; action: unknown };
	function rulesFor(): Rule[] {
		const a = step.operation["audio"] as { rules?: Rule[] } | undefined;
		return a?.rules ?? [];
	}
	function setRules(rules: Rule[]) {
		const a = { ...(step.operation["audio"] as Record<string, unknown> ?? {}) };
		a["rules"] = rules;
		step.operation = { ...step.operation, audio: a };
	}
	function addRule() {
		setRules([...rulesFor(), { match: { codecs: [], languages: [] }, action: "copy" }]);
	}
	function removeRule(i: number) {
		setRules(rulesFor().filter((_, j) => j !== i));
	}
	function ruleChip(i: number, v: string) {
		const rules = rulesFor().map((r) => ({ ...r, match: { ...r.match } }));
		const c = rules[i].match.codecs;
		const at = c.indexOf(v);
		if (at === -1) c.push(v);
		else c.splice(at, 1);
		setRules(rules);
	}
	function setRuleAction(i: number, action: unknown) {
		setRules(rulesFor().map((r, j) => (j === i ? { ...r, action } : r)));
	}
	function setRuleLanguages(i: number, text: string) {
		const rules = rulesFor().map((r) => ({ ...r, match: { ...r.match } }));
		rules[i].match.languages = text
			.split(",")
			.map((s) => s.trim())
			.filter(Boolean);
		setRules(rules);
	}

	// The audio section's policy fragment supplies the re-encode codec list.
	const reencodeCodecOpts = $derived.by(() => {
		const f = (schema.operation_sections["audio"]?.schema as { fields?: Record<string, { reencode?: { codec?: { values?: { value: string; label: string }[] } } }> })?.fields?.["default"];
		return f?.reencode?.codec?.values ?? [];
	});
	// Rule match-codec vocabulary from the audio section's rule item schema.
	const ruleMatchCodecOpts = $derived.by(() => {
		const f = (schema.operation_sections["audio"]?.schema as { fields?: Record<string, { item?: { match?: { codecs?: { values?: string[] } } } }> })?.fields?.["rules"];
		return f?.item?.match?.codecs?.values ?? [];
	});
	
	const noCondition = $derived.by(() => {
		// Object.keys alone does not subscribe on a $state proxy — read each
		// value so key add/remove re-runs this derived.
		const c = step.condition;
		const keys = Object.keys(c);
		for (const k of keys) void (c as Record<string, unknown>)[k];
		return keys.length === 0;
	});
</script>

<div class="flex flex-col gap-3 rounded-lg border p-4">
	<div class="flex items-center gap-2">
		<span class="rounded-md bg-muted px-2 py-0.5 font-mono text-xs">step {index + 1}</span>
		<span class="text-sm text-muted-foreground">
			{noCondition ? "matches every file" : "matches when all set fields apply"}
		</span>
		<div class="ml-auto flex gap-1">
			<Button variant="ghost" size="sm" onclick={onMoveUp} disabled={index === 0} title="Move up">
				<ChevronUpIcon class="size-4" />
			</Button>
			<Button variant="ghost" size="sm" onclick={onMoveDown} disabled={index === total - 1} title="Move down">
				<ChevronDownIcon class="size-4" />
			</Button>
			<Button variant="ghost" size="sm" onclick={onRemove} title="Delete step">
				<TrashIcon class="size-4" />
			</Button>
		</div>
	</div>

	<div class="grid gap-3 md:grid-cols-2">
		{#each Object.entries(schema.condition_fields) as [field, f] (field)}
			{#if f.schema.kind === "multi_select"}
				<div>
					<p class="mb-1 text-xs font-medium">{field}</p>
					<div class="flex flex-wrap gap-1">
						{#each f.schema.values as v (v)}
							<Button
								type="button"
								variant={listFor(field).includes(v) ? "default" : "outline"}
								size="sm"
								class="h-auto px-1.5 py-0.5 font-mono text-[11px]"
								onclick={() => toggleChip(field, v)}
							>
								{v}
							</Button>
						{/each}
					</div>
					<p class="mt-1 text-[11px] text-muted-foreground">{f.schema.hint ?? f.description}</p>
				</div>
			{:else if f.schema.kind === "resolution_range"}
				{@const r = resFor()}
				<div>
					<p class="mb-1 text-xs font-medium">resolution</p>
					<div class="flex flex-wrap items-center gap-1">
						{#each Object.entries(f.schema.common) as [label, wh] (label)}
							<Button
								type="button"
								variant={r.min?.[0] === wh[0] && r.min?.[1] === wh[1] ? "default" : "outline"}
								size="sm"
								class="h-auto px-1.5 py-0.5 text-[11px]"
								onclick={() => setRes("min", wh[0], wh[1])}
							>
								≥{label}
							</Button>
							<Button
								type="button"
								variant={r.max?.[0] === wh[0] && r.max?.[1] === wh[1] ? "default" : "outline"}
								size="sm"
								class="h-auto px-1.5 py-0.5 text-[11px]"
								onclick={() => setRes("max", wh[0], wh[1])}
							>
								≤{label}
							</Button>
						{/each}
					</div>
					<div class="mt-1 flex items-center gap-1 text-[11px] text-muted-foreground">
						min <Input type="number" class="h-6 w-16 px-1 text-[11px]" value={r.min?.[0] ? String(r.min[0]) : ""} oninput={(e) => setRes("min", Number((e.target as HTMLInputElement).value), Number(r.min?.[1] ?? 0))} placeholder="w" /> ×
						<Input type="number" class="h-6 w-16 px-1 text-[11px]" value={r.min?.[1] ? String(r.min[1]) : ""} oninput={(e) => setRes("min", Number(r.min?.[0] ?? 0), Number((e.target as HTMLInputElement).value))} placeholder="h" />
						max <Input type="number" class="h-6 w-16 px-1 text-[11px]" value={r.max?.[0] ? String(r.max[0]) : ""} oninput={(e) => setRes("max", Number((e.target as HTMLInputElement).value), Number(r.max?.[1] ?? 0))} placeholder="w" /> ×
						<Input type="number" class="h-6 w-16 px-1 text-[11px]" value={r.max?.[1] ? String(r.max[1]) : ""} oninput={(e) => setRes("max", Number(r.max?.[0] ?? 0), Number((e.target as HTMLInputElement).value))} placeholder="h" />
					</div>
				</div>
			{:else if f.schema.kind === "byte_range"}
				{@const s = sizeFor()}
				<div>
					<p class="mb-1 text-xs font-medium">file size (MB)</p>
					<div class="flex items-center gap-1">
						<Input type="number" min="0" class="h-8 w-24" value={s.min ? String(Math.round(s.min / 1_000_000)) : ""} oninput={(e) => setSize("min", (e.target as HTMLInputElement).value)} placeholder="min" />
						<span class="text-muted-foreground">→</span>
						<Input type="number" min="0" class="h-8 w-24" value={s.max ? String(Math.round(s.max / 1_000_000)) : ""} oninput={(e) => setSize("max", (e.target as HTMLInputElement).value)} placeholder="max" />
					</div>
				</div>
			{:else}
				<div>
					<p class="mb-1 text-xs font-medium">{field}</p>
					<FlowField schema={f.schema} bind:value={step.condition[field]} devices={devices} />
				</div>
			{/if}
		{/each}
	</div>

	<div class="border-t pt-3">
		<p class="mb-2 text-xs font-medium text-muted-foreground">Operation</p>
		<div class="flex flex-col gap-3">
			{#each Object.entries(schema.operation_sections) as [name, sec] (name)}
				{#if sec.schema.kind === "object"}
					{@const enabled = sectionEnabled(name)}
					<div class="flex flex-col gap-2 rounded-md border p-2">
						<div class="flex items-center gap-2">
						<SyncSwitch enabled={enabled} onSet={(on) => setSection(name, on)} />
							<span class="text-sm font-medium">{name}</span>
							{#if sec.schema.hint}
								<span class="text-[11px] text-muted-foreground">{sec.schema.hint}</span>
							{/if}
						</div>
						{#if enabled}
							<div class="grid gap-3 md:grid-cols-2">
								{#each Object.entries(sec.schema.fields) as [field, f] (field)}
									{#if name === "audio" && field === "rules"}
										<div class="md:col-span-2">
											<p class="mb-1 text-xs font-medium">Rules <span class="text-muted-foreground">(first match wins; re-encode applies to all matching tracks)</span></p>
											
											{#each rulesFor() as rule, i (i)}
												<div class="mb-2 flex flex-wrap items-center gap-2 rounded border p-2">
													<div class="flex flex-wrap gap-1">
												{#each ruleMatchCodecOpts as c (c)}
															<Button
																type="button"
																variant={rule.match.codecs.includes(c) ? "default" : "outline"}
																size="sm"
																class="h-auto px-1.5 py-0.5 font-mono text-[11px]"
																onclick={() => ruleChip(i, c)}
															>
																{c}
															</Button>
														{/each}
													</div>
													<Input class="h-7 w-32" placeholder="langs (en,fr)" value={rule.match.languages.join(", ")} oninput={(e) => setRuleLanguages(i, (e.target as HTMLInputElement).value)} />
													<span class="text-xs text-muted-foreground">→</span>
													{#if typeof rule.action === "string"}
														<select
															class="h-7 w-28 rounded-lg border border-input bg-transparent px-2 text-xs"
															value={rule.action}
																// "re-encode" writes the object form (AudioPolicy's re-encode is an object, not a string).
															onchange={(e) => {
																const v = (e.target as HTMLSelectElement).value;
																setRuleAction(i, v === "re-encode" ? { codec: "eac3", sample_rate: 48000, channels: 2 } : v);
															}}
														>
															<option value="copy">Copy</option>
															<option value="drop">Drop</option>
															<option value="re-encode">Re-encode</option>
														</select>
													{:else}
														<select
															class="h-7 w-28 rounded-lg border border-input bg-transparent px-2 text-xs"
															value={(rule.action as { codec?: string }).codec ?? "eac3"}
															onchange={(e) =>
																setRuleAction(i, {
																	codec: (e.target as HTMLSelectElement).value,
																	sample_rate: 48000,
																	channels: 2,
																})}
														>
														{#each reencodeCodecOpts as v (v.value)}
															<option value={v.value}>{v.label}</option>
														{/each}
														</select>
													{/if}
													<Button variant="ghost" size="sm" onclick={() => removeRule(i)}>
														<TrashIcon class="size-3.5" />
													</Button>
												</div>
											{/each}
											<Button variant="outline" size="sm" onclick={addRule}>Add rule</Button>
										</div>
									{:else}
										<div>
											<p class="mb-1 text-xs font-medium">{field}
												{#if f.hint}<span class="font-normal text-muted-foreground"> — {f.hint}</span>{/if}
											</p>
											<FlowField
												schema={f as UiSchema}
												bind:value={op[name][field]}
												devices={devices}
											/>
										</div>
									{/if}
								{/each}
							</div>
						{/if}
					</div>
				{:else}
					{@const enabled = sectionEnabled(name)}
					<div class="flex items-center gap-2">
					<SyncSwitch enabled={enabled} onSet={(on) => setSection(name, on)} />
						<span class="text-sm font-medium">{name}</span>
						{#if enabled}
							<FlowField
								schema={sec.schema}
								bind:value={op[name]}
								devices={devices}
								class="w-56"
							/>
						{/if}
					</div>
				{/if}
			{/each}
		</div>
	</div>
</div>
