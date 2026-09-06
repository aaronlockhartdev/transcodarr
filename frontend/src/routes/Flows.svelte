<script lang="ts">
	import { toast } from "svelte-sonner";
	import MoreHorizontalIcon from "@lucide/svelte/icons/more-horizontal";
	import * as Card from "$lib/components/ui/card";
	import * as Table from "$lib/components/ui/table";
	import * as Dialog from "$lib/components/ui/dialog";
	import * as Field from "$lib/components/ui/field";
	import { Input } from "$lib/components/ui/input";
	import { Button } from "$lib/components/ui/button";
	import * as Empty from "$lib/components/ui/empty";
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
	import WorkflowIcon from "@lucide/svelte/icons/workflow";
	import { api, ApiError } from "$lib/api.js";
	import { navigate } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";
	import { formatUnixSeconds } from "$lib/format.js";
	import type { FlowRecord } from "$lib/types.js";

	let createOpen = $state(false);
	let creating = $state(false);
	let newName = $state("");
	let startFrom = $state<number | "">("");
	let deleteTarget = $state<FlowRecord | null>(null);
	let deleteOpen = $state(false);
	let deleting = $state(false);

	async function createFlow() {
		const name = newName.trim();
		if (!name || creating) return;
		creating = true;
		try {
			let flowJson = '{"flow_version":1,"steps":[]}';
			if (startFrom !== "") {
				const src = await api.getFlow(startFrom as number);
				flowJson = src.flow_json;
			}
			const { id } = await api.createFlow({ name, flow_json: flowJson });
			createOpen = false;
			newName = "";
			startFrom = "";
			await store.refreshCore();
			navigate(`/flows/${id}`);
		} catch (e) {
			toast.error(e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));
		} finally {
			creating = false;
		}
	}

	async function confirmDelete() {
		if (!deleteTarget || deleting) return;
		deleting = true;
		try {
			await api.deleteFlow(deleteTarget.id);
			toast.success(`Deleted “${deleteTarget.name}”`);
			deleteOpen = false;
			await store.refreshCore();
		} catch (e) {
			toast.error(e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));
		} finally {
			deleting = false;
		}
	}
</script>

<div class="flex items-start justify-between gap-4">
	<div>
		<h1 class="text-2xl font-semibold tracking-tight">Flows</h1>
		<p class="text-muted-foreground">Shared format policies — one flow can serve many libraries.</p>
	</div>
	<Button onclick={() => (createOpen = true)}>New flow</Button>
</div>

{#if store.flows.length === 0}
	<Empty.Root class="my-16 flex-col">
		<Empty.Media variant="icon">
			<WorkflowIcon class="size-6" />
		</Empty.Media>
		<Empty.Title>No flows</Empty.Title>
		<Empty.Description>Create one, then assign it to as many libraries as you like.</Empty.Description>
		<Empty.Content>
			<Button onclick={() => (createOpen = true)}>New flow</Button>
		</Empty.Content>
	</Empty.Root>
{:else}
	<Card.Root class="mt-6">
		<Card.Content class="p-0">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>Name</Table.Head>
						<Table.Head class="text-right">Libraries</Table.Head>
						<Table.Head class="text-right">Updated</Table.Head>
						<Table.Head class="w-10" />
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each store.flows as f (f.id)}
						<Table.Row class="cursor-pointer" onclick={() => navigate(`/flows/${f.id}`)}>
							<Table.Cell class="font-medium">{f.name}</Table.Cell>
							<Table.Cell class="text-right tabular-nums">{f.library_count}</Table.Cell>
							<Table.Cell class="text-right text-muted-foreground">{formatUnixSeconds(f.updated_at)}</Table.Cell>
							<Table.Cell class="text-right">
								<DropdownMenu.Root>
									<DropdownMenu.Trigger class="rounded-md outline-none" onclick={(e) => e.stopPropagation()}>
										<MoreHorizontalIcon class="size-4" />
									</DropdownMenu.Trigger>
									<DropdownMenu.Content align="end" onclick={(e) => e.stopPropagation()}>
										<DropdownMenu.Group>
											<DropdownMenu.Item onclick={() => navigate(`/flows/${f.id}`)}>Edit</DropdownMenu.Item>
											<DropdownMenu.Separator />
											<DropdownMenu.Item variant="destructive" onclick={() => (deleteTarget = f, deleteOpen = true)}>
												Delete
											</DropdownMenu.Item>
										</DropdownMenu.Group>
									</DropdownMenu.Content>
								</DropdownMenu.Root>
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>
{/if}

<Dialog.Root bind:open={createOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>New flow</Dialog.Title>
			<Dialog.Description>A shared set of format rules, editable from the flow editor.</Dialog.Description>
		</Dialog.Header>
		<Field.FieldGroup>
			<Field.Field>
				<Field.FieldLabel for="flow-name">Name</Field.FieldLabel>
				<Input id="flow-name" bind:value={newName} placeholder="Plex-ready 1080p" />
			</Field.Field>
			<Field.Field>
				<Field.FieldLabel for="flow-source">Start from</Field.FieldLabel>
				<Field.FieldDescription>Blank starts empty; picking a flow copies its rules.</Field.FieldDescription>
				<select
					id="flow-source"
					class="h-8 w-full rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
					value={startFrom}
					onchange={(e) => {
						const v = (e.target as HTMLSelectElement).value;
						startFrom = v === "" ? "" : Number(v);
					}}
				>
					<option value="">Blank flow</option>
					{#each store.flows as f (f.id)}
						<option value={f.id}>{f.name}</option>
					{/each}
				</select>
			</Field.Field>
		</Field.FieldGroup>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (createOpen = false)}>Cancel</Button>
			<Button onclick={createFlow} disabled={!newName.trim() || creating}>
				{creating ? "Creating…" : "Create"}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root bind:open={deleteOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Delete “{deleteTarget?.name}”?</Dialog.Title>
			<Dialog.Description>
				{#if deleteTarget && deleteTarget.library_count > 0}
					{deleteTarget.library_count} librar{deleteTarget.library_count === 1 ? "y" : "ies"} use this flow — they will be
					unassigned and their files settle to unmatched.
				{:else}
					No libraries use this flow.
				{/if}
			</Dialog.Description>
		</Dialog.Header>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (deleteOpen = false)}>Cancel</Button>
			<Button variant="destructive" disabled={deleting} onclick={confirmDelete}>
				{deleting ? "Deleting…" : "Delete flow"}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
