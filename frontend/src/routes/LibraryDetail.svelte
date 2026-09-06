<script lang="ts">
	import { onMount, onDestroy } from "svelte";
	import { toast } from "svelte-sonner";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import HistoryIcon from "@lucide/svelte/icons/history";
	import ScanIcon from "@lucide/svelte/icons/scan";
	import * as Card from "$lib/components/ui/card";
	import * as Table from "$lib/components/ui/table";
	import * as Sheet from "$lib/components/ui/sheet";
	import * as Tooltip from "$lib/components/ui/tooltip";
	import { Badge } from "$lib/components/ui/badge";
	import { Button } from "$lib/components/ui/button";
	import * as Empty from "$lib/components/ui/empty";
	import LibraryFormDialog from "$lib/components/LibraryFormDialog.svelte";
	import StatusBadge from "$lib/components/StatusBadge.svelte";
	import { api } from "$lib/api.js";
	import { navigate } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";
	import { basename, formatBytes, formatUnixSeconds, parseFacts } from "$lib/format.js";
	import type { FileRow } from "$lib/types.js";
	import { cn } from "$lib/utils";
	import { sortBy } from "$lib/sort";
	import SortableHead from "$lib/components/tables/sortable-head.svelte";
	import { Input } from "$lib/components/ui/input";

	let { id }: { id: number } = $props();

	const lib = $derived(store.libraries.find((l) => l.id === id));
	const files = $derived(store.filesOf(id));

	// Poll this library's files while the screen is open.
	let poll: ReturnType<typeof setInterval> | null = null;
	onMount(() => {
		void store.refreshLibraryFiles(id);
		poll = setInterval(() => void store.refreshLibraryFiles(id), 5000);
	});
	onDestroy(() => poll && clearInterval(poll));

	const summary = $derived.by(() => {
		const count = (...statuses: string[]) => files.filter((f) => statuses.includes(f.status)).length;
		return {
			total: files.length,
			compliant: count("compliant", "completed"),
			queued: count("queued"),
			active: count("running", "verifying", "scanning"),
			failed: count("failed", "quarantined"),
			unmatched: count("unmatched"),
			unscanned: count("unscanned"),
		};
	});

	let scanning = $state(false);
	async function scanNow() {
		scanning = true;
		try {
			const r = await api.scanLibrary(id);
			toast.success(`Scanned ${r.scanned} files — queued ${r.queued}`);
			await store.refreshLibraryFiles(id);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
		} finally {
			scanning = false;
		}
	}

	// File → job history drawer (DESIGN §9.2).
	let historyFile = $state<FileRow | null>(null);
	let historyOpen = $state(false);
	const fileJobs = $derived.by(() => {
		const h = historyFile;
		if (!h) return [];
		return store.jobs.filter((j) => j.file_id === h.id).sort((a, b) => b.id - a.id);
	});

	let editOpen = $state(false);

	function resolutionOf(f: FileRow): string | null {
		const facts = parseFacts(f.facts_json);
		if (facts?.video?.width && facts?.video?.height) {
			return `${facts.video.width}×${facts.video.height}`;
		}
		return null;
	}
	function audioOf(f: FileRow): string | null {
		const facts = parseFacts(f.facts_json);
		if (!facts?.audio?.length) return null;
		return facts.audio
			.map((t) => (t.atmos ? `${t.codec}·Atmos` : t.codec))
			.join(", ");
	}
	function deltaOf(f: FileRow): number | null {
		if (f.input_size != null && f.output_size != null) return f.output_size - f.input_size;
		return null;
	}

	// ---- table filter & sort ---------------------------------------------
	const STATUS_GROUPS: Record<string, string[]> = {
		files: ["compliant", "completed", "queued", "running", "verifying", "scanning", "failed", "quarantined", "unmatched", "unscanned"],
		compliant: ["compliant", "completed"],
		queued: ["queued"],
		active: ["running", "verifying", "scanning"],
		failed: ["failed", "quarantined"],
		unmatched: ["unmatched"],
	};
	let query = $state("");
	let statusFilter = $state<string | null>(null);
	let sortKey = $state<"name" | "size" | "delta" | "checked" | null>(null);
	let sortDir = $state<"asc" | "desc">("asc");
	function toggleSort(k: typeof sortKey) {
		if (k === null) return;
		if (sortKey === k) sortDir = sortDir === "asc" ? "desc" : "asc";
		else {
			sortKey = k;
			sortDir = "asc";
		}
	}
	const shown = $derived.by(() => {
		const q = query.trim().toLowerCase();
		const group = statusFilter ? STATUS_GROUPS[statusFilter] : null;
		let list = files.filter(
			(f) =>
				(q === "" || f.path.toLowerCase().includes(q)) &&
				(group === null || group.includes(f.status)),
		);
		if (sortKey === null) return list;
		list = sortBy(
			list,
			(f) =>
				sortKey === "name"
					? basename(f.path).toLowerCase()
					: sortKey === "size"
						? f.size
						: sortKey === "delta"
							? deltaOf(f) ?? 0
							: f.last_probed ?? 0,
			sortDir,
		);
		return list;
	});
	const badgeDefs = $derived([
		{ key: "files", label: "files", count: summary.total, variant: "secondary" as const, dimmed: false },
		{ key: "compliant", label: "compliant", count: summary.compliant, variant: "secondary" as const, dimmed: false },
		{ key: "queued", label: "queued", count: summary.queued, variant: "outline" as const, dimmed: false },
		{ key: "active", label: "active", count: summary.active, variant: "default" as const, dimmed: false },
		{ key: "failed", label: "failed", count: summary.failed, variant: "destructive" as const, dimmed: summary.failed === 0 },
		{ key: "unmatched", label: "unmatched", count: summary.unmatched, variant: "outline" as const, dimmed: summary.unmatched === 0 },
	]);
</script>

{#if !lib}
	<Empty.Root class="my-16 flex-col">
		<Empty.Title>Library not found</Empty.Title>
		<Empty.Content>
			<Button variant="outline" onclick={() => navigate("/libraries")}>
				<ChevronLeftIcon class="size-4" data-icon="inline-start" />
				Back to libraries
			</Button>
		</Empty.Content>
	</Empty.Root>
{:else}
	<button class="mb-2 flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground" onclick={() => navigate("/libraries")}>
		<ChevronLeftIcon class="size-4" data-icon="inline-start" />
		Libraries
	</button>

	<div class="flex items-start justify-between gap-4">
		<div class="min-w-0">
			<div class="flex items-center gap-3">
				<h1 class="text-2xl font-semibold tracking-tight">{lib.name}</h1>
				{#if lib.watchable}
					<Badge variant="outline">watched</Badge>
				{:else}
					<Badge variant="secondary">scan-driven</Badge>
				{/if}
				<Badge variant="secondary">{lib.lifecycle_mode}</Badge>
			</div>
			<p class="truncate font-mono text-sm text-muted-foreground">{lib.path}</p>
			<p class="text-sm text-muted-foreground">
				Flow: {#if lib.flow_name}
					<button
						class="underline decoration-dotted underline-offset-2 hover:text-foreground"
						onclick={() => navigate(`/flows/${lib.flow_id}`)}
					>
						{lib.flow_name}
					</button>
				{:else}
					<span>none</span>
				{/if}
			</p>
			<div class="mt-2 flex flex-wrap items-center gap-1.5 text-sm">
				{#each badgeDefs as b (b.key)}
					<button
						class={cn(
							"rounded-full",
							statusFilter === b.key ? "ring-2 ring-ring" : "hover:opacity-80",
						)}
						onclick={() => (statusFilter = statusFilter === b.key ? null : b.key)}
						title={b.key === "files" ? "Show all files" : `Show only ${b.label} files`}
					>
						<Badge variant={b.variant} class={b.dimmed ? "opacity-40" : ""}>{b.count} {b.label}</Badge>
					</button>
				{/each}
			</div>
		</div>
		<div class="flex shrink-0 flex-col items-end gap-2">
			<div class="flex gap-2">
				<Button variant="outline" onclick={() => (editOpen = true)}>Edit library</Button>
				<Button onclick={scanNow} disabled={scanning}>
					<ScanIcon class="size-4" data-icon="inline-start" />
					{scanning ? "Scanning…" : "Scan now"}
				</Button>
			</div>
			<Input class="h-9 w-64" placeholder="Search files…" bind:value={query} />
		</div>
	</div>

	<Card.Root class="mt-6">
		<Card.Content class="p-0">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<SortableHead active={sortKey === "name"} dir={sortDir} onToggle={() => toggleSort("name")}>
							File
						</SortableHead>
						<Table.Head>Resolution</Table.Head>
						<Table.Head>Video</Table.Head>
						<Table.Head>Audio</Table.Head>
						<Table.Head>Status</Table.Head>
						<SortableHead class="text-right" active={sortKey === "size"} dir={sortDir} onToggle={() => toggleSort("size")}>
							Size
						</SortableHead>
						<SortableHead class="text-right" active={sortKey === "delta"} dir={sortDir} onToggle={() => toggleSort("delta")}>
							Δ after transcode
						</SortableHead>
						<SortableHead active={sortKey === "checked"} dir={sortDir} onToggle={() => toggleSort("checked")}>
							Last checked
						</SortableHead>
						<Table.Head class="w-10" />
					</Table.Row>
				</Table.Header>
				<Table.Body>
					{#each shown as f (f.id)}
						{@const facts = parseFacts(f.facts_json)}
						{@const delta = deltaOf(f)}
						<Table.Row>
							<Table.Cell class="max-w-80">
								<div class="truncate font-medium" title={f.path}>{basename(f.path)}</div>
								<div class="truncate font-mono text-xs text-muted-foreground" title={f.path}>{f.path}</div>
							</Table.Cell>
							<Table.Cell class="whitespace-nowrap">{resolutionOf(f) ?? "—"}</Table.Cell>
							<Table.Cell class="whitespace-nowrap font-mono text-xs">{facts?.video?.codec ?? "—"}</Table.Cell>
							<Table.Cell class="max-w-44 truncate font-mono text-xs" title={audioOf(f) ?? ""}>{audioOf(f) ?? "—"}</Table.Cell>
							<Table.Cell><StatusBadge status={f.status} /></Table.Cell>
							<Table.Cell class="whitespace-nowrap text-right tabular-nums">{formatBytes(f.size)}</Table.Cell>
							<Table.Cell class="whitespace-nowrap text-right tabular-nums {delta != null && delta < 0 ? 'text-emerald-600' : ''}">
								{delta == null ? "—" : `${delta < 0 ? "−" : "+"}${formatBytes(Math.abs(delta))}`}
							</Table.Cell>
							<Table.Cell class="whitespace-nowrap text-muted-foreground">{formatUnixSeconds(f.last_probed)}</Table.Cell>
							<Table.Cell class="text-right">
								<Tooltip.Root>
									<Tooltip.Trigger>
										{#snippet child({ props })}
											<button
												class="inline-flex size-7 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
												{...props}
												onclick={() => (historyFile = f, historyOpen = true)}
											>
												<span class="sr-only">Job history for {basename(f.path)}</span>
												<HistoryIcon class="size-4" data-icon="inline-start" />
											</button>
										{/snippet}
									</Tooltip.Trigger>
									<Tooltip.Content side="top">Job history</Tooltip.Content>
								</Tooltip.Root>
							</Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row>
							<Table.Cell class="py-10 text-center text-muted-foreground" colspan={9}>
								{files.length === 0 ? "No files yet — run a scan." : "No files match."}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</Card.Content>
	</Card.Root>
{/if}

<LibraryFormDialog lib={lib ?? null} bind:open={editOpen} onSaved={() => void store.refreshCore()} />

<Sheet.Root bind:open={historyOpen}>
	<Sheet.Content>
		<Sheet.Header>
			<Sheet.Title>Job history — {historyFile ? basename(historyFile.path) : ""}</Sheet.Title>
			<Sheet.Description>Most recent first.</Sheet.Description>
		</Sheet.Header>
		<Sheet.Content class="pt-0">
			{#if fileJobs.length === 0}
				<p class="py-6 text-center text-sm text-muted-foreground">No jobs for this file.</p>
			{:else}
				<ul class="flex flex-col divide-y">
					{#each fileJobs as j (j.id)}
						<li>
							<button
								class="flex w-full items-center gap-3 py-2.5 text-left text-sm hover:opacity-80"
								onclick={() => navigate(`/jobs/${j.id}`)}
							>
								<StatusBadge status={j.state} />
								<span class="min-w-0 flex-1">
									<span class="block truncate">{formatUnixSeconds(j.started ?? j.ended)}</span>
									<span class="block truncate font-mono text-xs text-muted-foreground">
										{[j.exit_kind, j.device_id].filter(Boolean).join(" · ") || `job #${j.id}`}
									</span>
								</span>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</Sheet.Content>
	</Sheet.Content>
</Sheet.Root>
