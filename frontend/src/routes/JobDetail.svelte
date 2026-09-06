<script lang="ts">
	import { toast } from "svelte-sonner";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import RefreshIcon from "@lucide/svelte/icons/refresh-cw";
	import * as Card from "$lib/components/ui/card";
	import * as Empty from "$lib/components/ui/empty";
	import { Badge } from "$lib/components/ui/badge";
	import { Button } from "$lib/components/ui/button";
	import StatusBadge from "$lib/components/StatusBadge.svelte";
	import { api } from "$lib/api.js";
	import { navigate } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";
	import { sse, subscribeJobLog } from "$lib/events.svelte.js";
	import { basename, formatDuration, formatUnixSeconds } from "$lib/format.js";

	let { id }: { id: number } = $props();

	const job = $derived(store.jobs.find((j) => j.id === id));
	const file = $derived.by(() => {
		if (!job) return undefined;
		for (const files of Object.values(store.filesByLibrary)) {
			const hit = files.find((f) => f.id === job.file_id);
			if (hit) return hit;
		}
		return undefined;
	});

	let log = $state<string | null>(null);
	let loadingLog = $state(false);
	let live = $state(false);
	let logBox = $state<HTMLDivElement | null>(null);
	let autoScroll = $state(true);

	async function refreshLog() {
		loadingLog = true;
		try {
			log = await api.jobLog(id);
			if (logBox) logBox.scrollTop = logBox.scrollHeight;
		} catch {
			log = "(no log available)";
		} finally {
			loadingLog = false;
		}
	}
	void refreshLog();

	const inFlight = $derived(job?.state === "running" || job?.state === "verifying");
	const liveMode = $derived(!!inFlight && sse.connected);

	function appendChunk(chunk: string) {
		if (!chunk) return;
		const cur = log ?? "";
		// A chunk's first line may duplicate the last line already
		// shown (a batch boundary can straddle a line).
		const lastLine = cur.split("\n").pop() ?? "";
		const firstLine = chunk.split("\n")[0] ?? "";
		const out =
			lastLine && firstLine && lastLine === firstLine ? chunk.slice(firstLine.length + 1) : chunk;
		log = cur + out;
		if (autoScroll && logBox) logBox.scrollTop = logBox.scrollHeight;
	}

	// While the job is in flight and the stream is up, append
	// `job_log` chunks live; on exit, a one-shot fetch takes the
	// authoritative archived tail.
	let liveKey = "";
	$effect(() => {
		const key = `${id}:${inFlight ? 1 : 0}:${sse.connected ? 1 : 0}`;
		if (key === liveKey) return;
		liveKey = key;
		if (live && !liveMode) void refreshLog();
		live = liveMode;
		if (live) {
			void api
				.jobLog(id)
				.then((t) => {
					log = t;
					if (logBox) logBox.scrollTop = logBox.scrollHeight;
				})
				.catch(() => {
					log ??= "";
				});
			return subscribeJobLog(id, appendChunk);
		}
	});

	// This page owns one job: keep its row current independently of
	// the list nudges, so any field (state, exit_kind, times) converges
	// within one interval even if a nudge was lost. On the transition
	// to a terminal state, take the final authoritative log.
	let lastState: string | null = null;
	$effect(() => {
		const st = job?.state ?? null;
		if (st !== lastState) {
			lastState = st;
			if (st === "completed" || st === "failed") void refreshLog();
		}
	});
	// In-flight only: while a job is running this page keeps its row
	// fresh with a light per-job fetch (the list nudges cover the
	// terminal transition; this covers everything in between).
	$effect(() => {
		if (!job || job.state === "completed" || job.state === "failed") return;
		const t = setInterval(() => {
			void api
				.getJob(id)
				.then((fresh) => {
					const i = store.jobs.findIndex((j) => j.id === id);
					if (i >= 0) store.jobs[i] = fresh;
				})
				.catch(() => {});
		}, 3000);
		return () => clearInterval(t);
	});

	const plan = $derived.by(() => {
		if (!job) return null;
		try {
			return JSON.stringify(JSON.parse(job.plan_json), null, 2);
		} catch {
			return null;
		}
	});
	const duration = $derived.by(() => {
		if (!job || job.started == null) return null;
		const end = job.ended ?? Math.floor(Date.now() / 1000);
		return formatDuration(end - job.started);
	});
	const device = $derived(store.devices.find((d) => d.id === job?.device_id));
</script>

{#if !job}
	<Empty.Root class="my-16 flex-col">
		<Empty.Title>Job #{id} not found</Empty.Title>
		<Empty.Content>
			<Button variant="outline" onclick={() => navigate("/jobs")}>
				<ChevronLeftIcon class="size-4" data-icon="inline-start" />
				Back to jobs
			</Button>
		</Empty.Content>
	</Empty.Root>
{:else}
	<button class="mb-2 flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground" onclick={() => navigate("/jobs")}>
		<ChevronLeftIcon class="size-4" data-icon="inline-start" />
		Jobs
	</button>

	<div class="flex flex-wrap items-center gap-3">
		<h1 class="text-2xl font-semibold tracking-tight">Job #{job.id}</h1>
		<StatusBadge status={job.state} />
		{#if device}
			<Badge variant="outline">{device.name}</Badge>
		{/if}
		{#if (job.state === "completed" || job.state === "failed") && job.exit_kind}
			<Badge variant="secondary" class="font-mono">{job.exit_kind}</Badge>
		{/if}
	</div>

	{#if file}
		<button
			class="mt-1 block max-w-full truncate font-mono text-sm text-muted-foreground hover:text-foreground"
			title={file.path}
			onclick={() => navigate(`/libraries/${job.library_id}`)}
		>
			{file.path}
		</button>
	{/if}

	<div class="mt-6 grid gap-4 lg:grid-cols-2">
		<Card.Root>
			<Card.Header>
				<Card.Title>Details</Card.Title>
			</Card.Header>
			<Card.Content>
				<dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
					<dt class="text-muted-foreground">Library</dt>
					<dd>{store.libraries.find((l) => l.id === job.library_id)?.name ?? `#${job.library_id}`}</dd>
					<dt class="text-muted-foreground">File</dt>
					<dd class="truncate font-mono" title={file?.path}>{file ? basename(file.path) : `#${job.file_id}`}</dd>
					<dt class="text-muted-foreground">Flow version</dt>
					<dd>v{job.flow_version}</dd>
					<dt class="text-muted-foreground">Started</dt>
					<dd>{formatUnixSeconds(job.started)}</dd>
					<dt class="text-muted-foreground">Ended</dt>
					<dd>{formatUnixSeconds(job.ended)}</dd>
					<dt class="text-muted-foreground">Duration</dt>
					<dd>{duration ?? "—"}</dd>
					{#if job.log_path}
						<dt class="text-muted-foreground">Log</dt>
						<dd class="truncate font-mono text-xs" title={job.log_path}>{job.log_path}</dd>
					{/if}
					{#if job.quarantine_path}
						<dt class="text-muted-foreground">Quarantine</dt>
						<dd class="truncate font-mono text-xs" title={job.quarantine_path}>{job.quarantine_path}</dd>
					{/if}
				</dl>
			</Card.Content>
		</Card.Root>

		<Card.Root>
			<Card.Header>
				<Card.Title>Log tail</Card.Title>
				<Card.Description>Last 200 lines of the ffmpeg log</Card.Description>
			</Card.Header>
			<Card.Content>
				<div class="flex items-center justify-between gap-2">
					{#if live}
						<Badge variant="secondary" class="gap-1.5">
							<span class="size-1.5 rounded-full bg-emerald-500 animate-pulse"></span>
							live
						</Badge>
					{:else if job.log_path}
						<p class="text-xs text-muted-foreground">{job.log_path}</p>
					{:else}
						<p class="text-xs text-muted-foreground">no log yet</p>
					{/if}
					<Button variant="ghost" size="sm" onclick={refreshLog} disabled={loadingLog}>
						<RefreshIcon data-icon="inline-start" class="size-3.5 {loadingLog ? 'animate-spin' : ''}" />
						Refresh
					</Button>
				</div>
				<div
					class="mt-2 h-64 overflow-y-auto rounded-md border p-3"
					bind:this={logBox}
					onscroll={() => {
						if (logBox) autoScroll = logBox.scrollHeight - logBox.scrollTop - logBox.clientHeight < 24;
					}}
				>
					<pre class="font-mono text-xs leading-5 whitespace-pre-wrap">{log ?? "loading…"}</pre>
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root class="lg:col-span-2">
			<Card.Header>
				<Card.Title>Plan</Card.Title>
				<Card.Description>The FfmpegPlan this job executes (device resolved at dispatch time)</Card.Description>
			</Card.Header>
			<Card.Content>
			<div class="max-h-80 overflow-y-auto rounded-md border p-3">
				<pre class="font-mono text-xs leading-5">{plan ?? "(no plan recorded)"}</pre>
			</div>
			</Card.Content>
		</Card.Root>
	</div>
{/if}
