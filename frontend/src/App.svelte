<script lang="ts">
	import { onMount } from "svelte";
	import { toast } from "svelte-sonner";
	import { Toaster } from "$lib/components/ui/sonner";
	import * as Sidebar from "$lib/components/ui/sidebar";
	import AppSidebar from "$lib/components/AppSidebar.svelte";
	import Dashboard from "$routes/Dashboard.svelte";
	import Libraries from "$routes/Libraries.svelte";
	import LibraryDetail from "$routes/LibraryDetail.svelte";
	import FlowEditor from "$routes/FlowEditor.svelte";
	import Jobs from "$routes/Jobs.svelte";
	import JobDetail from "$routes/JobDetail.svelte";
	import Settings from "$routes/Settings.svelte";
	import { parseRoute, usePath } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";

	const route = $derived(parseRoute(usePath()));

	onMount(() => {
		store.startPolling();
		return () => store.stopPolling();
	});

	// Surface (and dedupe) server errors as toasts.
	let lastToast = $state<string | null>(null);
	$effect(() => {
		const err = store.fetchError;
		if (err && err !== lastToast) {
			lastToast = err;
			toast.error(`server: ${err}`);
		}
	});
</script>

<!-- The page canvas (bg-sidebar: a light gray in light mode, the darkest
     surface in dark mode) shows through the gap around the two cards, which
     is what separates the bar from the content. -->
<div class="flex h-svh w-full overflow-hidden bg-sidebar dark:bg-background">
	<Sidebar.Provider>
		<AppSidebar />
		<Sidebar.Inset>
			<div class="min-h-0 flex-1 overflow-y-auto p-6">
				{#if route.name === "dashboard"}
					<Dashboard />
				{:else if route.name === "libraries"}
					<Libraries />
				{:else if route.name === "library"}
					<LibraryDetail id={Number(route.id)} />
				{:else if route.name === "flow"}
					<FlowEditor id={Number(route.id)} />
				{:else if route.name === "jobs"}
					<Jobs />
				{:else if route.name === "job"}
					<JobDetail id={Number(route.id)} />
				{:else if route.name === "settings"}
					<Settings />
				{:else}
					<div class="flex h-full items-center justify-center text-muted-foreground">Not found.</div>
				{/if}
			</div>
		</Sidebar.Inset>
	</Sidebar.Provider>
</div>

<Toaster />
