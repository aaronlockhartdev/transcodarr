<script lang="ts">
	import HouseIcon from "@lucide/svelte/icons/house";
	import FilmIcon from "@lucide/svelte/icons/film";
	import ListTodoIcon from "@lucide/svelte/icons/list-todo";
	import SettingsIcon from "@lucide/svelte/icons/settings";
	import * as Sidebar from "$lib/components/ui/sidebar";
	import * as Tooltip from "$lib/components/ui/tooltip";
	import { Button } from "$lib/components/ui/button";
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
	];

	// Live badge on the Jobs item while jobs are active.
	const activeJobs = $derived(
		store.jobs.filter((j) => j.state === "running" || j.state === "verifying").length,
	);

	const settingsActive = $derived(path.startsWith("/settings"));
</script>

<Sidebar.Root variant="inset">
	<Sidebar.Header>
		<!-- Wordmark only — the icon comes later (kept out on purpose). -->
		<div class="px-2 py-1.5 text-sm font-semibold tracking-tight">Transcodarr</div>
	</Sidebar.Header>

	<Sidebar.Content>
		<Sidebar.Group>
			<Sidebar.GroupContent>
				<Sidebar.Menu>
					{#each items as item (item.title)}
						<Sidebar.MenuItem>
							<Sidebar.MenuButton isActive={item.match(path)} tooltipContent={item.title}>
								{#snippet child({ props })}
									<a
										href={item.url}
										{...props}
										onclick={(e) => {
											e.preventDefault();
											navigate(item.url);
										}}
									>
										<item.icon class="size-4" />
										<span>{item.title}</span>
										{#if item.title === "Jobs" && activeJobs > 0}
											<span
												class="ml-auto rounded-full bg-sidebar-accent px-1.5 text-xs tabular-nums text-sidebar-accent-foreground"
											>
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
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<Tooltip.Root>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<Button
								variant="ghost"
								size="icon"
								{...props}
								aria-label="Settings"
								class={cn(settingsActive && "bg-sidebar-accent text-sidebar-accent-foreground")}
								onclick={() => navigate("/settings")}
							>
								<SettingsIcon class="size-4" />
							</Button>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Content side="right">Settings</Tooltip.Content>
				</Tooltip.Root>
			</Sidebar.MenuItem>
		</Sidebar.Menu>
		{#if store.health}
			<p class="px-2 pb-1 text-xs text-muted-foreground">flow v{store.health.flow_version}</p>
		{/if}
	</Sidebar.Footer>
</Sidebar.Root>
