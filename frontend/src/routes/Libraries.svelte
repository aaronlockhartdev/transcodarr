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
	import { sortBy } from "$lib/sort";
	import SortableHead from "$lib/components/tables/sortable-head.svelte";
	import { Input } from "$lib/components/ui/input";

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

	let query = $state("");
	let sortKey = $state<"name" | "files" | "compliant" | "failed" | "size" | null>(null);
	let sortDir = $state<"asc" | "desc">("asc");
	function toggleSort(k: typeof sortKey) {
		if (k === null) return;
		if (sortKey === k) sortDir = sortDir === "asc" ? "desc" : "asc";
		else {
			sortKey = k;
			sortDir = "asc";
		}
	}
	const rows = $derived.by(() => {
		const q = query.trim().toLowerCase();
		const libs = store.libraries.filter(
			(l) => q === "" || l.name.toLowerCase().includes(q) || l.path.toLowerCase().includes(q),
		);
		const withSum = libs.map((lib) => ({ lib, s: summary(lib) }));
		if (sortKey === null) return withSum;
		return sortBy(
			withSum,
			(r) =>
				sortKey === "name"
					? r.lib.name
					: sortKey === "files"
						? r.s.total
						: sortKey === "compliant"
							? r.s.compliant
							: sortKey === "failed"
								? r.s.failed
								: r.s.bytes,
			sortDir,
		);
	});

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
	<div class="flex items-center gap-2">
		<Input class="h-9 w-52" placeholder="Search libraries…" bind:value={query} />
		<Button onclick={() => (createOpen = true)}>New library</Button>
	</div>
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
						<SortableHead active={sortKey === "name"} dir={sortDir} onToggle={() => toggleSort("name")}>
							Library
						</SortableHead>
						<Table.Head>Path</Table.Head>
						<Table.Head>Flow</Table.Head>
						<Table.Head>Mode</Table.Head>
						<SortableHead class="text-right" active={sortKey === "files"} dir={sortDir} onToggle={() => toggleSort("files")}>
							Files
						</SortableHead>
						<SortableHead class="text-right" active={sortKey === "compliant"} dir={sortDir} onToggle={() => toggleSort("compliant")}>
							Compliant
						</SortableHead>
						<SortableHead class="text-right" active={sortKey === "failed"} dir={sortDir} onToggle={() => toggleSort("failed")}>
							Failed
						</SortableHead>
						<SortableHead class="text-right" active={sortKey === "size"} dir={sortDir} onToggle={() => toggleSort("size")}>
							Size
						</SortableHead>
						<Table.Head class="w-10" />
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each rows as { lib, s } (lib.id)}
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
							<Table.Cell>
							{#if lib.flow_name}
								<button
									class="underline decoration-dotted underline-offset-2 hover:text-foreground"
									onclick={(e) => {
										e.stopPropagation();
										navigate(`/flows/${lib.flow_id}`);
									}}
								>
									{lib.flow_name}
								</button>
							{:else}
								<span class="text-muted-foreground">none</span>
							{/if}
						</Table.Cell>
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
											<DropdownMenu.Separator />
											<DropdownMenu.Item variant="destructive" onclick={() => (deleting = lib, deleteOpen = true)}>
												Delete
											</DropdownMenu.Item>
										</DropdownMenu.Group>
									</DropdownMenu.Content>
								</DropdownMenu.Root>
							</Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row>
							<Table.Cell colspan={9} class="text-center text-muted-foreground">No libraries match.</Table.Cell>
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
