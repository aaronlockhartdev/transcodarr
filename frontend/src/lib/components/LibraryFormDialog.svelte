<script lang="ts">
	import { toast } from "svelte-sonner";
	import * as Dialog from "$lib/components/ui/dialog";
	import * as Field from "$lib/components/ui/field";
	import { Input } from "$lib/components/ui/input";
	import { Switch } from "$lib/components/ui/switch";
	import { Button } from "$lib/components/ui/button";
	import { api, ApiError } from "$lib/api.js";
	import { store } from "$lib/store.svelte.js";
	import type { Library } from "$lib/types.js";

	let {
		lib,
		open = $bindable(),
		onOpenChange,
		onSaved,
	}: {
		/** null → create mode */
		lib: Library | null;
		open?: boolean;
		onOpenChange?: (open: boolean) => void;
		onSaved?: (id: number) => void;
	} = $props();

	const empty = {
		name: "",
		path: "",
		lifecycle_mode: "manual",
		flow_id: null as number | null,
		auto_queue: true,
		retention_days: 7,
		auto_delete: false,
		scan_schedule: "",
		watchable: false,
	};

	function libData() {
		return lib
			? {
					name: lib.name,
					path: lib.path,
					lifecycle_mode: lib.lifecycle_mode,
					flow_id: lib.flow_id ?? null,
					auto_queue: lib.auto_queue,
					retention_days: lib.retention_days,
					auto_delete: lib.auto_delete,
					scan_schedule: lib.scan_schedule ?? "",
					watchable: lib.watchable,
				}
			: {};
	}

	const form = $state({ ...empty, ...libData() });

	// The dialog's open state changes which library is edited; re-seed the
	// whole form when it opens (or the caller swaps `lib` while it is open).
	$effect(() => {
		if (open) {
			form.name = lib?.name ?? "";
			form.path = lib?.path ?? "";
			form.lifecycle_mode = lib?.lifecycle_mode ?? "manual";
			form.flow_id = lib?.flow_id ?? null;
			form.auto_queue = lib?.auto_queue ?? true;
			form.retention_days = lib?.retention_days ?? 7;
			form.auto_delete = lib?.auto_delete ?? false;
			form.scan_schedule = lib?.scan_schedule ?? "";
			form.watchable = lib?.watchable ?? false;
		}
	});

	let saving = $state(false);
	let error = $state<string | null>(null);

	function close() {
		open = false;
		onOpenChange?.(false);
	}

	async function save() {
		if (!form.name.trim() || !form.path.trim()) {
			error = "Name and path are required.";
			return;
		}
		saving = true;
		error = null;
		try {
			const body = {
				name: form.name.trim(),
				path: form.path.trim(),
				lifecycle_mode: form.lifecycle_mode,
				flow_id: form.flow_id,
				auto_queue: form.auto_queue,
				retention_days: Number(form.retention_days) || 0,
				auto_delete: form.auto_delete,
				scan_schedule: form.scan_schedule.trim() || null,
				watchable: form.watchable,
			};
			if (lib) {
				await api.updateLibrary({ ...body, id: lib.id });
				toast.success(`Saved “${body.name}”`);
				close();
				onSaved?.(lib.id);
			} else {
				const { id } = await api.createLibrary(body);
				toast.success(`Created “${body.name}”`);
				close();
				onSaved?.(id);
			}
		} catch (e) {
			error = e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog.Root bind:open onOpenChange={(v) => onOpenChange?.(v)}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>{lib ? `Edit ${lib.name}` : "New library"}</Dialog.Title>
			<Dialog.Description>
				{lib
					? "Saving triggers an immediate re-evaluation of the library's files."
					: "A library is a folder tree of media files to keep format-compliant."}
			</Dialog.Description>
		</Dialog.Header>
		<div class="pt-0">
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="lib-name">Name</Field.FieldLabel>
					<Input id="lib-name" bind:value={form.name} placeholder="Movies" />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="lib-path">Path</Field.FieldLabel>
					<Input id="lib-path" bind:value={form.path} placeholder="/media/movies" class="font-mono text-sm" />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="lib-flow">Flow</Field.FieldLabel>
					<Field.FieldDescription>Flows are shared objects — edit the selected one on the Flows page.</Field.FieldDescription>
					<select
						id="lib-flow"
						class="h-8 w-full rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
						value={form.flow_id ?? ""}
						onchange={(e) => {
							const v = (e.target as HTMLSelectElement).value;
							form.flow_id = v === "" ? null : Number(v);
						}}
					>
						<option value="">No flow (files settle to unmatched)</option>
						{#each store.flows as f (f.id)}
							<option value={f.id}>{f.name}</option>
						{/each}
					</select>
				</Field.Field>
				<div class="grid grid-cols-2 gap-4">
					<Field.Field>
						<Field.FieldLabel for="lib-lifecycle">Lifecycle</Field.FieldLabel>
						<select
							id="lib-lifecycle"
							class="h-8 w-full rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
							value={form.lifecycle_mode}
							onchange={(e) => (form.lifecycle_mode = (e.target as HTMLSelectElement).value)}
						>
							<option value="manual">Manual (verify before replacing)</option>
							<option value="auto">Auto</option>
						</select>
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="lib-retention">Quarantine retention (days)</Field.FieldLabel>
						<Input
							id="lib-retention"
							type="number"
							min="0"
							bind:value={form.retention_days}
						/>
					</Field.Field>
				</div>
				<Field.Field>
					<Field.FieldLabel for="lib-scan">Scan schedule</Field.FieldLabel>
					<Field.FieldDescription>Optional; blank = on demand only.</Field.FieldDescription>
					<Input id="lib-scan" bind:value={form.scan_schedule} placeholder="blank" class="font-mono text-sm" />
				</Field.Field>
				<div class="flex items-center justify-between gap-4">
					<div class="flex flex-col gap-0.5">
						<span class="text-sm font-medium">Auto-queue</span>
						<span class="text-sm text-muted-foreground">Queue non-conforming files after a scan</span>
					</div>
					<Switch bind:checked={form.auto_queue} />
				</div>
				<div class="flex items-center justify-between gap-4">
					<div class="flex flex-col gap-0.5">
						<span class="text-sm font-medium">Auto-delete</span>
						<span class="text-sm text-muted-foreground">Delete the original after a verified transcode (irreversible)</span>
					</div>
					<Switch bind:checked={form.auto_delete} />
				</div>
				<div class="flex items-center justify-between gap-4">
					<div class="flex flex-col gap-0.5">
						<span class="text-sm font-medium">Watchable</span>
						<span class="text-muted-foreground">Local filesystem: file-watch triggers in addition to scans</span>
					</div>
					<Switch bind:checked={form.watchable} />
				</div>
				{#if error}
					<p class="text-sm text-destructive" role="alert">{error}</p>
				{/if}
			</Field.FieldGroup>
		</div>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => close()}>Cancel</Button>
			<Button onclick={save} disabled={saving}>
				{saving ? "Saving…" : lib ? "Save" : "Create library"}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
