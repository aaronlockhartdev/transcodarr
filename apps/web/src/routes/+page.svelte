<script lang="ts">
	import { api, type components } from '#lib/api/client.ts';

	type Job = components['schemas']['Job'];

	let jobs = $state<Job[]>([]);
	let error = $state<string | null>(null);
	let path = $state('');

	async function load() {
		const { data, error: err } = await api.GET('/jobs');
		if (err) {
			error = JSON.stringify(err);
			return;
		}
		jobs = data ?? [];
	}

	async function submit() {
		const { data } = await api.POST('/jobs', { body: { path, preset: null } });
		if (data) jobs = [...jobs, data];
	}

	async function cancel(id: number) {
		await api.DELETE('/jobs/{id}', { params: { path: { id } } });
		await load();
	}
</script>

<h1>transcodarr</h1>

<input bind:value={path} placeholder="/library/movie.mkv" />
<button onclick={submit}>enqueue</button>
<button onclick={load}>refresh</button>

{#if error}
	<p role="alert">{error}</p>
{/if}

<ul>
	{#each jobs as j (j.id)}
		<li>
			{j.path} &mdash; {j.state} {Math.round(j.progress * 100)}%
			{#if j.fail_reason}({j.fail_reason}){/if}
			<button onclick={() => cancel(j.id)}>cancel</button>
		</li>
	{/each}
</ul>
