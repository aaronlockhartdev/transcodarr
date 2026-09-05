<script lang="ts">
	import FilmIcon from "@lucide/svelte/icons/film";
	import { scaleBand } from "d3-scale";
	import { BarChart } from "layerchart";
	import * as Chart from "$lib/components/ui/chart";
	import * as Card from "$lib/components/ui/card";
	import * as Empty from "$lib/components/ui/empty";
	import { Button } from "$lib/components/ui/button";
	import { Progress } from "$lib/components/ui/progress";
	import { Badge } from "$lib/components/ui/badge";
	import StatusBadge from "$lib/components/StatusBadge.svelte";
	import { navigate } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";
	import { formatBytes, formatRelative } from "$lib/format.js";
	import type { FileRow, JobRow } from "$lib/types.js";

	const COMPLIANT = new Set(["compliant", "completed"]);

	// ── Derived stats (all from existing tables — DESIGN §9.1) ─────
	const allFiles = $derived(
		store.libraries.flatMap((l) => store.filesOf(l.id))
	);

	const fileById = $derived(
		Object.fromEntries(allFiles.map((f) => [f.id, f] as [number, FileRow])),
	);
	const totalFiles = $derived(allFiles.length);
	const compliantFiles = $derived(allFiles.filter((f) => COMPLIANT.has(f.status)).length);
	const compliancePct = $derived(
		totalFiles === 0 ? null : Math.round((compliantFiles / totalFiles) * 1000) / 10,
	);
	const storageBytes = $derived(allFiles.reduce((n, f) => n + f.size, 0));
	const spaceSavedBytes = $derived(
		allFiles.reduce(
			(n, f) => (f.input_size != null && f.output_size != null ? n + Math.max(0, f.input_size - f.output_size) : n),
			0,
		),
	);
	const failedFiles = $derived(allFiles.filter((f) => f.status === "failed" || f.status === "quarantined").length);

	// ── Files completed per day, last 30 days (job history) ────────
	const chartData = $derived.by(() => {
		const byDay = new Map<string, { files: number; bytes: number }>();
		for (const j of store.jobs) {
			if (j.state !== "completed" || j.ended == null) continue;
			const d = new Date(j.ended * 1000);
			// Full date key (year included) so MM-DD never collides across years.
			const iso = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
			const cutoff = new Date();
			cutoff.setHours(0, 0, 0, 0);
			cutoff.setDate(cutoff.getDate() - 29);
			if (d < cutoff) continue;
			const key = `${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
			const slot = byDay.get(iso) ?? { files: 0, bytes: 0 };
			const file = fileById[j.file_id];
			slot.files += 1;
			slot.bytes += file && file.input_size != null && file.output_size != null
				? Math.max(0, file.input_size - file.output_size)
				: 0;
			byDay.set(iso, slot);
		}
		return [...byDay.entries()]
			.map(([iso, v]) => ({ day: iso.slice(5), iso, files: v.files, bytes: v.bytes }))
			.sort((a, b) => a.iso.localeCompare(b.iso))
			.slice(-30);
	});
	const chartConfig = {
		files: { label: "files completed", color: "var(--chart-1)" },
	} satisfies Chart.ChartConfig;

	// ── Per-device active vs cap ───────────────────────────────────
	const perDevice = $derived(
		store.devices.map((d) => {
			const active = store.jobs.filter(
				(j) => (j.state === "running" || j.state === "verifying") && j.device_id === d.id,
			).length;
			return { device: d, active };
		}),
	);

	// ── Recent failures + oldest waiting ──────────────────────────
	const recentFailures = $derived(
		store.jobs
			.filter((j) => j.state === "failed" || j.state === "quarantined")
			.sort((a, b) => (b.ended ?? 0) - (a.ended ?? 0))
			.slice(0, 5),
	);
	const oldestWaiting = $derived(
		store.jobs
			.filter((j) => j.state === "queued")
			.sort((a, b) => a.id - b.id)
			.slice(0, 5),
	);

	function fileOf(j: JobRow): FileRow | undefined {
		return fileById[j.file_id];
	}
</script>

<h1 class="mb-1 text-2xl font-semibold tracking-tight">Dashboard</h1>
<p class="mb-6 text-muted-foreground">Library compliance at a glance.</p>

{#if store.libraries.length === 0}
	<Empty.Root class="my-16 flex-col">
		<Empty.Media variant="icon"><FilmIcon class="size-6" /></Empty.Media>
		<Empty.Title>No libraries yet</Empty.Title>
		<Empty.Description>Point Transcodarr at a media collection to start making it format-compliant.</Empty.Description>
		<Empty.Content>
			<Button onclick={() => navigate("/libraries")}>
				<FilmIcon class="size-4" data-icon="inline-start" />
				Add a library
			</Button>
		</Empty.Content>
	</Empty.Root>
{:else}
	<!-- Stat cards -->
	<div class="grid grid-cols-2 gap-4 xl:grid-cols-5">
		<Card.Root>
			<Card.Header>
				<Card.Description>Files</Card.Description>
				<Card.Title>{totalFiles.toLocaleString()}</Card.Title>
			</Card.Header>
		</Card.Root>
		<Card.Root>
			<Card.Header>
				<Card.Description>Compliant</Card.Description>
				<Card.Title>{compliancePct === null ? "—" : `${compliancePct}%`}</Card.Title>
			</Card.Header>
		</Card.Root>
		<Card.Root>
			<Card.Header>
				<Card.Description>Library storage</Card.Description>
				<Card.Title>{formatBytes(storageBytes)}</Card.Title>
			</Card.Header>
		</Card.Root>
		<Card.Root>
			<Card.Header>
				<Card.Description>Space saved by transcoding</Card.Description>
				<Card.Title>{formatBytes(spaceSavedBytes)}</Card.Title>
			</Card.Header>
		</Card.Root>
		<Card.Root>
			<Card.Header>
				<Card.Description>Failed / quarantined</Card.Description>
				<Card.Title class={failedFiles > 0 ? "text-destructive" : ""}>{failedFiles}</Card.Title>
			</Card.Header>
		</Card.Root>
	</div>

	<div class="mt-6 grid gap-4 lg:grid-cols-2">
		<!-- Files completed per day -->
		<Card.Root>
			<Card.Header>
				<Card.Title>Completed, last 30 days</Card.Title>
				<Card.Description>Files per day from job history</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if chartData.length === 0}
					<p class="py-8 text-center text-sm text-muted-foreground">Nothing completed yet.</p>
				{:else}
					<Chart.Container config={chartConfig} class="min-h-[220px] w-full">
						<BarChart
							data={chartData}
							xScale={scaleBand().padding(0.3)}
							x="day"
							axis="x"
							series={[{ key: "files", label: chartConfig.files.label, color: chartConfig.files.color }]}
						>
							{#snippet tooltip()}
								<Chart.Tooltip />
							{/snippet}
						</BarChart>
					</Chart.Container>
				{/if}
			</Card.Content>
		</Card.Root>

		<!-- Per-device active vs cap -->
		<Card.Root>
			<Card.Header>
				<Card.Title>Devices</Card.Title>
				<Card.Description>Active jobs vs per-device cap</Card.Description>
			</Card.Header>
			<Card.Content>
				<div class="flex flex-col gap-4">
					{#each perDevice as { device, active } (device.id)}
						<div class="flex flex-col gap-1.5">
							<div class="flex items-center justify-between text-sm">
								<span class="flex items-center gap-2">
									{device.name}
									<Badge variant="outline" class="font-mono text-xs uppercase">{device.kind}</Badge>
								</span>
								<span class="text-muted-foreground">{active} / {device.max_concurrent}</span>
							</div>
							<Progress value={(active / Math.max(1, device.max_concurrent)) * 100} class="h-1.5" />
						</div>
					{/each}
				</div>
			</Card.Content>
		</Card.Root>

		<!-- Recent failures -->
		<Card.Root>
			<Card.Header>
				<Card.Title>Recent failures</Card.Title>
				<Card.Description>Jump to a job for its log</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if recentFailures.length === 0}
					<p class="py-4 text-sm text-muted-foreground">No failures. 🎉</p>
				{:else}
					<ul class="flex flex-col divide-y">
						{#each recentFailures as j (j.id)}
							<li>
								<button
									class="flex w-full items-center gap-3 py-2 text-left text-sm hover:opacity-80"
									onclick={() => navigate(`/jobs/${j.id}`)}
								>
									<StatusBadge status={j.state} />
									<span class="min-w-0 flex-1 truncate">{fileOf(j)?.path ?? `file ${j.file_id}`}</span>
									{#if j.exit_kind}<span class="shrink-0 font-mono text-xs text-muted-foreground">{j.exit_kind}</span>{/if}
									<span class="shrink-0 text-xs text-muted-foreground">{formatRelative(j.ended)}</span>
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</Card.Content>
		</Card.Root>

		<!-- Oldest waiting -->
		<Card.Root>
			<Card.Header>
				<Card.Title>Oldest waiting</Card.Title>
				<Card.Description>Longest-queued jobs (FIFO)</Card.Description>
			</Card.Header>
			<Card.Content>
				{#if oldestWaiting.length === 0}
					<p class="py-4 text-sm text-muted-foreground">Queue is empty.</p>
				{:else}
					<ul class="flex flex-col divide-y">
						{#each oldestWaiting as j (j.id)}
							<li>
								<button
									class="flex w-full items-center gap-3 py-2 text-left text-sm hover:opacity-80"
									onclick={() => navigate(`/jobs/${j.id}`)}
								>
									<StatusBadge status={j.state} />
									<span class="min-w-0 flex-1 truncate">{fileOf(j)?.path ?? `file ${j.file_id}`}</span>
									<span class="shrink-0 text-xs text-muted-foreground">job #{j.id}</span>
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</Card.Content>
		</Card.Root>
	</div>
{/if}
