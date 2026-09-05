<script lang="ts">
	import HouseIcon from "@lucide/svelte/icons/house";
	import FilmIcon from "@lucide/svelte/icons/film";
	import WorkflowIcon from "@lucide/svelte/icons/workflow";
	import ListTodoIcon from "@lucide/svelte/icons/list-todo";
	import SettingsIcon from "@lucide/svelte/icons/settings";
	import * as Sidebar from "$lib/components/ui/sidebar";
	import { cn } from "$lib/utils.js";
	import { navigate, usePath } from "$lib/router.svelte.js";
	import { store } from "$lib/store.svelte.js";

	const path = $derived(usePath());

	const items = [
		{ title: "Dashboard", url: "/", icon: HouseIcon, match: (p: string) => p === "/" },
		{
			title: "Libraries",
			url: "/libraries",
			icon: FilmIcon,
			match: (p: string) => p.startsWith("/libraries") || p.startsWith("/flows"),
		},
		{
			title: "Jobs",
			url: "/jobs",
			icon: ListTodoIcon,
			match: (p: string) => p.startsWith("/jobs"),
		},
		{
			title: "Settings",
			url: "/settings",
			icon: SettingsIcon,
			match: (p: string) => p.startsWith("/settings"),
		},
	];

	// Live badge on the Jobs item while jobs are active.
	const activeJobs = $derived(
		store.jobs.filter((j) => j.state === "running" || j.state === "verifying").length,
	);
</script>

<Sidebar.Root variant="inset">
	<Sidebar.Header>
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<a href="/" class="flex items-center gap-2 font-semibold" onclick={(e) => { e.preventDefault(); navigate("/"); }}>
					<WorkflowIcon class="size-5" />
					<span>Transcodarr</span>
				</a>
			</Sidebar.MenuItem>
		</Sidebar.Menu>
	</Sidebar.Header>
	<Sidebar.Content>
		<Sidebar.Group>
			<Sidebar.GroupLabel>Application</Sidebar.GroupLabel>
			<Sidebar.GroupContent>
				<Sidebar.Menu>
					{#each items as item (item.title)}
						<Sidebar.MenuItem>
							<Sidebar.MenuButton>
								{#snippet child({ props })}
									<a
										href={item.url}
										{...props}
										class={cn(item.match(path) && "bg-sidebar-accent text-sidebar-accent-foreground")}
										data-active={item.match(path) || undefined}
										onclick={(e) => {
											e.preventDefault();
											navigate(item.url);
										}}
									>
										<item.icon />
										<span>{item.title}</span>
										{#if item.title === "Jobs" && activeJobs > 0}
											<span class="ml-auto flex size-5 items-center justify-center rounded-full bg-primary text-xs text-primary-foreground">
												{activeJobs}
											</span>
										{/if}
									</a>
								{/snippet}
							</Sidebar.MenuButton>
						</Sidebar.MenuItem>
					{/each}
				</Sidebar.Menu>
			</Sidebar.GroupContent>
		</Sidebar.Group>
	</Sidebar.Content>
	<Sidebar.Footer>
		{#if store.health}
			<p class="px-2 text-xs text-muted-foreground">flow v{store.health.flow_version}</p>
		{/if}
	</Sidebar.Footer>
</Sidebar.Root>
