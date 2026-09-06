<script lang="ts">
	import * as Table from "$lib/components/ui/table";
	import * as Tabs from "$lib/components/ui/tabs";
	import * as Card from "$lib/components/ui/card";
	import StatusBadge from "$lib/components/StatusBadge.svelte";
	import { store } from "$lib/store.svelte.js";
	import { navigate } from "$lib/router.svelte.js";
	import { basename, formatDuration, formatRelative, formatUnixSeconds } from "$lib/format.js";
	import type { FileRow, JobRow } from "$lib/types.js";
	import { sortBy } from "$lib/sort";
	import SortableHead from "$lib/components/tables/sortable-head.svelte";
	import { Input } from "$lib/components/ui/input";

	const TABS = [
		{ value: "all", label: "All" },
		{ value: "active", label: "Active" },
		{ value: "queued", label: "Queued" },
		{ value: "failed", label: "Failed" },
		{ value: "completed", label: "Completed" },
	] as const;

	let tab = $state<string>("all");

	function matches(value: string, j: JobRow): boolean {
		switch (value) {
			case "active":
				return j.state === "running" || j.state === "verifying";
			case "queued":
				return j.state === "queued";
			case "failed":
				return j.state === "failed" || j.state === "quarantined" || j.state === "canceled";
			case "completed":
				return j.state === "completed";
			default:
				return true;
		}
	}

	let query = $state("");
	let sortKey = $state<"started" | "duration" | null>(null);
	let sortDir = $state<"asc" | "desc">("asc");
	function toggleSort(k: typeof sortKey) {
		if (k === null) return;
		if (sortKey === k) sortDir = sortDir === "asc" ? "desc" : "asc";
		else {
			sortKey = k;
			sortDir = "desc"; // newest/longest first is the useful default
		}
	}
	const visible = $derived.by(() => {
		const q = query.trim().toLowerCase();
		let list = store.jobs
			.filter((j) => matches(tab, j))
			.filter((j) => q === "" || fileOf(j)?.path.toLowerCase().includes(q));
		if (sortKey === null) {
			// Default order: newest first.
			list = [...list].sort((a, b) => (b.started ?? 0) - (a.started ?? 0));
			return list.slice(0, 200);
		}
		list = sortBy(
			list,
			(j) =>
				sortKey === "started"
					? j.started ?? 0
					: durSecs(j) ?? -1,
			sortDir,
		);
		return list.slice(0, 200);
	});

	const activeCount = $derived(
		store.jobs.filter((j) => j.state === "running" || j.state === "verifying").length,
	);

	function countOf(value: string) {
		return store.jobs.filter((j) => matches(value, j)).length;
	}

	function fileOf(j: JobRow): FileRow | undefined {
		// Fast path: the job's own library. The full walk below only runs
		// when that library's files are not in the store yet.
		if (j.library_id != null) {
			const hit = store.filesByLibrary[j.library_id]?.find((f) => f.id === j.file_id);
			if (hit) return hit;
		}
		for (const files of Object.values(store.filesByLibrary)) {
			const hit = files.find((f) => f.id === j.file_id);
			if (hit) return hit;
		}
		return undefined;
	}
	function durSecs(j: JobRow): number | null {
		if (j.started != null && (j.ended != null || j.state === "running" || j.state === "verifying")) {
			const end = j.ended ?? Math.floor(Date.now() / 1000);
			return end - j.started;
		}
		return null;
	}
	function durationOf(j: JobRow): string | null {
		const s = durSecs(j);
		return s == null ? null : formatDuration(s);
	}
</script>

<div class="flex items-start justify-between gap-4">
	<div>
		<h1 class="text-2xl font-semibold tracking-tight">Jobs</h1>
		<p class="text-muted-foreground">
			Transcode jobs, newest first.{activeCount > 0
				? ` ${activeCount} active right now.`
				: ""}
		</p>
	</div>
	<Input class="h-9 w-56" placeholder="Search by file…" bind:value={query} />
</div>

<Card.Root class="mt-6">
	<Card.Content class="p-0">
		<Tabs.Root bind:value={tab} class="flex flex-col">
			<div class="border-b px-4 py-3">
				<Tabs.List>
					{#each TABS as t (t.value)}
						<Tabs.Trigger value={t.value}>
							{t.label}
							<span class="ml-1.5 text-xs text-muted-foreground">{countOf(t.value)}</span>
						</Tabs.Trigger>
					{/each}
				</Tabs.List>
			</div>
			{#each TABS as t (t.value)}
				<Tabs.Content value={t.value} class="p-0">
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head>Job</Table.Head>
								<Table.Head>File</Table.Head>
								<Table.Head>State</Table.Head>
								<Table.Head>Device</Table.Head>
								<Table.Head>Exit</Table.Head>
								<SortableHead active={sortKey === "started"} dir={sortDir} onToggle={() => toggleSort("started")}>
									Started
								</SortableHead>
								<SortableHead class="text-right" active={sortKey === "duration"} dir={sortDir} onToggle={() => toggleSort("duration")}>
									Duration
								</SortableHead>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each visible as j (j.id)}
								{@const f = fileOf(j)}
								<Table.Row class="cursor-pointer" onclick={() => navigate(`/jobs/${j.id}`)}>
									<Table.Cell class="font-mono text-xs">#{j.id}</Table.Cell>
									<Table.Cell class="max-w-96">
										<span class="block truncate font-medium" title={f?.path}>{f ? basename(f.path) : `file ${j.file_id}`}</span>
										<span class="block truncate font-mono text-xs text-muted-foreground" title={f?.path}>
											{f?.path ?? ""}
										</span>
									</Table.Cell>
									<Table.Cell><StatusBadge status={j.state} /></Table.Cell>
									<Table.Cell class="font-mono text-xs">{j.device_id ?? "—"}</Table.Cell>
									<Table.Cell class="font-mono text-xs text-muted-foreground">{j.exit_kind ?? "—"}</Table.Cell>
									<Table.Cell class="whitespace-nowrap text-muted-foreground">
										{formatUnixSeconds(j.started)}
										{#if j.started == null}<span class="text-xs">({formatRelative(j.lease_expires)} lease)</span>{/if}
									</Table.Cell>
									<Table.Cell class="whitespace-nowrap text-right tabular-nums">{durationOf(j) ?? "—"}</Table.Cell>
								</Table.Row>
							{:else}
								<Table.Row>
									<Table.Cell class="py-10 text-center text-muted-foreground" colspan={7}>No jobs in this view.</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</Tabs.Content>
			{/each}
		</Tabs.Root>
	</Card.Content>
</Card.Root>
