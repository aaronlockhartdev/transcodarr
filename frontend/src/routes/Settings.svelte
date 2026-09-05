<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import * as Card from "$lib/components/ui/card";
	import * as Table from "$lib/components/ui/table";
	import { Badge } from "$lib/components/ui/badge";
	import { Button } from "$lib/components/ui/button";
	import { Input } from "$lib/components/ui/input";
	import { api } from "$lib/api.js";
	import { store } from "$lib/store.svelte.js";

	const KEYS_STORE = "transcodarr.settings.keys";

	let settings = $state<Record<string, string>>({});
	let newKey = $state("");
	let newValue = $state("");
	let savingKey = $state<string | null>(null);

	function knownKeys(): string[] {
		try {
			return JSON.parse(localStorage.getItem(KEYS_STORE) ?? "[]") as string[];
		} catch {
			return [];
		}
	}
	function rememberKeys() {
		localStorage.setItem(KEYS_STORE, JSON.stringify(Object.keys(settings)));
	}

	onMount(async () => {
		// The server has no list-settings endpoint, so keys we have seen are
		// remembered locally and their values re-read on load.
		for (const k of knownKeys()) {
			const r = await api.getSetting(k).catch(() => null);
			if (r && r.value != null) settings[k] = r.value;
		}
		rememberKeys();
	});

	async function saveSetting(key: string, value: string) {
		savingKey = key;
		try {
			await api.setSetting(key, value);
			settings[key] = value;
			rememberKeys();
			toast.success(`Saved ${key}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
		} finally {
			savingKey = null;
		}
	}
	async function deleteSetting(key: string) {
		try {
			await api.setSetting(key, "");
			delete settings[key];
			rememberKeys();
			toast.success(`Cleared ${key}`);
		} catch (e) {
			toast.error(e instanceof Error ? e.message : String(e));
		}
	}
	async function addSetting() {
		const k = newKey.trim();
		if (!k) return;
		await saveSetting(k, newValue);
		newKey = "";
		newValue = "";
	}

	const running = $derived(store.jobs.filter((j) => j.state === "running" || j.state === "verifying"));
	const busyCount = (id: string) => running.filter((j) => j.device_id === id).length;
</script>

<h1 class="text-2xl font-semibold tracking-tight">Settings</h1>
<p class="text-muted-foreground">Server health, encoder devices, and the free-form settings store.</p>

<div class="mt-6 grid gap-4 lg:grid-cols-2">
	<Card.Root>
		<Card.Header>
			<Card.Title>Server</Card.Title>
		</Card.Header>
		<Card.Content>
			{#if store.health}
				<dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
					<dt class="text-muted-foreground">Status</dt>
					<dd>
						<Badge variant="secondary">{store.health.status}</Badge>
					</dd>
					<dt class="text-muted-foreground">Flow version</dt>
					<dd>v{store.health.flow_version}</dd>
					<dt class="text-muted-foreground">Devices</dt>
					<dd>{store.devices.length}</dd>
				</dl>
			{:else}
				<p class="text-sm text-muted-foreground">No health data — is the API reachable?</p>
			{/if}
			<p class="mt-4 text-xs text-muted-foreground">
				v1 binds to 127.0.0.1 by default with no built-in authentication — put it behind an
				authenticating reverse proxy if the port must be exposed.
			</p>
		</Card.Content>
	</Card.Root>

	<Card.Root>
		<Card.Header>
			<Card.Title>Settings store</Card.Title>
			<Card.Description>Free-form key/value pairs. v1 reads none by default — ffmpeg/ffprobe paths are CLI flags.</Card.Description>
		</Card.Header>
		<Card.Content>
			{#if Object.keys(settings).length === 0}
				<p class="text-sm text-muted-foreground">No settings stored yet.</p>
			{:else}
				<ul class="flex flex-col gap-2">
					{#each Object.entries(settings) as [k, v] (k)}
						<li class="flex items-center gap-2">
							<span class="w-48 shrink-0 truncate font-mono text-xs" title={k}>{k}</span>
							<Input
								class="flex-1"
								value={v}
								onchange={(e) => void saveSetting(k, (e.target as HTMLInputElement).value)}
							/>
							<Button
								variant="ghost"
								size="sm"
								disabled={savingKey === k}
								onclick={() => void deleteSetting(k)}
							>
								Clear
							</Button>
						</li>
					{/each}
				</ul>
			{/if}
			<div class="mt-4 flex gap-2 border-t pt-4">
				<Input
					placeholder="new key"
					bind:value={newKey}
					class="w-48"
				/>
				<Input
					placeholder="value"
					bind:value={newValue}
					class="flex-1"
				/>
				<Button onclick={addSetting} disabled={!newKey.trim()}>Add</Button>
			</div>
		</Card.Content>
	</Card.Root>
</div>

<div class="mt-6 rounded-lg border">
	<div class="border-b px-4 py-3">
		<h2 class="font-semibold">Devices</h2>
		<p class="text-sm text-muted-foreground">
			Detected at startup with a real test-encode; a device is listed only if at least one of its encoders works.
		</p>
	</div>
	<Table.Root>
		<Table.Header>
			<Table.Row>
				<Table.Head>Device</Table.Head>
				<Table.Head>Type</Table.Head>
				<Table.Head>Encoders</Table.Head>
				<Table.Head class="text-right">Max concurrent</Table.Head>
				<Table.Head class="text-right">Busy</Table.Head>
			</Table.Row>
		</Table.Header>
		<Table.Body>
			{#each store.devices as d (d.id)}
				<Table.Row>
					<Table.Cell>
						<div class="font-medium">{d.name}</div>
						<div class="font-mono text-xs text-muted-foreground">{d.id}</div>
					</Table.Cell>
					<Table.Cell>
						<Badge variant={d.kind === "gpu" ? "default" : "secondary"}>{d.kind}</Badge>
					</Table.Cell>
					<Table.Cell>
						<div class="flex flex-wrap gap-1">
							{#each d.encoders as e (e.name)}
								<Badge variant="outline" class="font-mono">{e.name}</Badge>
							{/each}
						</div>
					</Table.Cell>
					<Table.Cell class="text-right tabular-nums">{d.max_concurrent}</Table.Cell>
					<Table.Cell class="text-right tabular-nums">{busyCount(d.id)}</Table.Cell>
				</Table.Row>
			{:else}
				<Table.Row>
					<Table.Cell class="py-10 text-center text-muted-foreground" colspan={5}>
						No devices detected (a CPU fallback always exists on the server).
					</Table.Cell>
				</Table.Row>
			{/each}
		</Table.Body>
	</Table.Root>
</div>
