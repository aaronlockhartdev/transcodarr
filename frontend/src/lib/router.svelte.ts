// A minimal history-based router for the embedded SPA.
//
// The production origin is the Rust binary, which serves the SPA fallback for
// any non-API path — so HTML5 history navigation (no hash) is safe: a refresh
// on /libraries/3 hits the fallback and this module resumes at that path.

let current = $state(window.location.pathname);

function onPopState() {
	current = window.location.pathname;
}
window.addEventListener("popstate", onPopState);

/** Push a new history entry and switch the route. */
export function navigate(to: string) {
	if (to === current) return;
	window.history.pushState({}, "", to);
	current = to;
	window.scrollTo(0, 0);
}

/** The live route path (reactive in components). */
export function usePath() {
	return current;
}

export interface Route {
	name:
		| "dashboard"
		| "libraries"
		| "library"
		| "jobs"
		| "job"
		| "settings"
		| "flow"
		| "notfound";
	id?: string;
}

export function parseRoute(path: string): Route {
	const seg = path.split("/").filter(Boolean);
	switch (seg[0] ?? "") {
		case "":
			return { name: "dashboard" };
		case "libraries":
			return seg[1] ? { name: "library", id: seg[1] } : { name: "libraries" };
		case "jobs":
			return seg[1] ? { name: "job", id: seg[1] } : { name: "jobs" };
		case "settings":
			return { name: "settings" };
		case "flows":
			return seg[1] ? { name: "flow", id: seg[1] } : { name: "libraries" };
		default:
			return { name: "notfound" };
	}
}
