<script lang="ts">
	import FilmIcon from "@lucide/svelte/icons/film";
	import MoreHorizontalIcon from "@lucide/svelte/icons/more-horizontal";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card";
	import * as Table from "$lib/components/ui/table";
	import * as Dialog from "$lib/components/ui/dialog";
	import { Badge } from "$lib/components/ui/badge";
	import { Button } from "$lib/components/ui/button";
	import * as Empty from "$lib/components/ui/empty";
	import * as DropdownMenu from "$lib/components/ui/dropdown-menu";
	import LibraryFormDialog from "$lib/components/LibraryFormDialog.svelte";
	import { api } from "$lib/api.js";
	import { navigate } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";
	import { formatBytes } from "$lib/format.js";
	import type { Library } from "$lib/types.js";

	let createOpen = $state(false);
	let editing = $state<Library | null>(null);
	let editOpen = $state(false);
	let deleting = $state<Library | null>(null);
	let deleteOpen = $state(false);
	let deletingBusy = $state(false);

	// Per-library compliance summary (DESIGN §9.2).
	function summary(lib: Library) {
		const files = store.filesOf(lib.id);
		const count = (...statuses: string[]) => files.filter((f) => statuses.includes(f.status)).length;
		return {
			total: files.length,
			compliant: count("compliant", "completed"),
			failed: count("failed", "quarantined"),
			bytes: files.reduce((n, f) => n + f.size, 0),
		};
	}

	function flowName(lib: Library): string | null {
		try {
			const f = JSON.parse(lib.flow_json);
			return typeof f?.name === "string" && f.name.length > 0 ? f.name : null;
		} catch {
			return null;
		}
	}

	async function confirmDelete() {
		if (!deleting) return;
		deletingBusy = true;
		try {
			await api.deleteLibrary(deleting.id);
			toast.success(`Deleted “${deleting.name}”`);
			deleteOpen = false;
			await store.refreshCore();
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
		} finally {
			deletingBusy = false;
		}
	}
</script>

<div class="flex items-start justify-between gap-4">
	<div>
		<h1 class="text-2xl font-semibold tracking-tight">Libraries</h1>
		<p class="text-muted-foreground">Media collections under management.</p>
	</div>
	<Button onclick={() => (createOpen = true)}>New library</Button>
</div>

{#if store.libraries.length === 0}
	<Empty.Root class="my-16 flex-col">
		<Empty.Media variant="icon">
			<FilmIcon class="size-6" />
		</Empty.Media>
		<Empty.Title>No libraries</Empty.Title>
		<Empty.Description>Create one to point Transcodarr at a media collection.</Empty.Description>
		<Empty.Content>
			<Button onclick={() => (createOpen = true)}>New library</Button>
		</Empty.Content>
	</Empty.Root>
{:else}
	<Card.Root class="mt-6">
		<Card.Content class="p-0">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>Library</Table.Head>
						<Table.Head>Path</Table.Head>
						<Table.Head>Flow</Table.Head>
						<Table.Head>Mode</Table.Head>
						<Table.Head class="text-right">Files</Table.Head>
						<Table.Head class="text-right">Compliant</Table.Head>
						<Table.Head class="text-right">Failed</Table.Head>
						<Table.Head class="text-right">Size</Table.Head>
						<Table.Head class="w-10" />
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each store.libraries as lib (lib.id)}
						{@const s = summary(lib)}
						<Table.Row class="cursor-pointer" onclick={() => navigate(`/libraries/${lib.id}`)}>
							<Table.Cell class="font-medium">
								{lib.name}
								{#if lib.watchable}
									<Badge variant="outline" class="ml-2 font-normal">watched</Badge>
								{:else}
									<Badge variant="secondary" class="ml-2 font-normal">scan-driven</Badge>
								{/if}
							</Table.Cell>
							<Table.Cell class="max-w-56 truncate font-mono text-xs text-muted-foreground">{lib.path}</Table.Cell>
							<Table.Cell>{#if flowName(lib)}{flowName(lib)}{:else}<span class="text-muted-foreground">(default)</span>{/if}</Table.Cell>
							<Table.Cell><Badge variant="secondary">{lib.lifecycle_mode}</Badge></Table.Cell>
							<Table.Cell class="text-right tabular-nums">{s.total}</Table.Cell>
							<Table.Cell class="text-right tabular-nums {s.compliant < s.total ? '' : 'text-emerald-700 dark:text-emerald-400'}">{s.compliant}</Table.Cell>
							<Table.Cell class="text-right tabular-nums {s.failed > 0 ? 'text-destructive' : ''}">{s.failed}</Table.Cell>
							<Table.Cell class="text-right tabular-nums">{formatBytes(s.bytes)}</Table.Cell>
							<Table.Cell class="text-right">
								<DropdownMenu.Root>
									<DropdownMenu.Trigger class="rounded-md outline-none" onclick={(e) => e.stopPropagation()}>
										<MoreHorizontalIcon class="size-4" />
									</DropdownMenu.Trigger>
									<DropdownMenu.Content align="end" onclick={(e) => e.stopPropagation()}>
										<DropdownMenu.Group>
											<DropdownMenu.Item
												onclick={() => {
													editing = lib;
													editOpen = true;
												}}
											>
												Edit
											</DropdownMenu.Item>
											<DropdownMenu.Item onclick={() => navigate(`/flows/${lib.id}`)}>
												Edit flow
											</DropdownMenu.Item>
											<DropdownMenu.Separator />
											<DropdownMenu.Item variant="destructive" onclick={() => (deleting = lib, deleteOpen = true)}>
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

<LibraryFormDialog lib={null} bind:open={createOpen} onSaved={() => void store.refreshCore()} />
<LibraryFormDialog lib={editing} bind:open={editOpen} onSaved={() => void store.refreshCore()} />

<Dialog.Root bind:open={deleteOpen}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Delete “{deleting?.name}”?</Dialog.Title>
			<Dialog.Description>
				This removes the library and its file/job history from Transcodarr. The media files on disk are not
				touched.
			</Dialog.Description>
		</Dialog.Header>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => (deleteOpen = false)}>Cancel</Button>
			<Button variant="destructive" disabled={deletingBusy} onclick={confirmDelete}>
				{deletingBusy ? "Deleting…" : "Delete library"}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
