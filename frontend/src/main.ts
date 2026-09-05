import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// Follow the OS light/dark preference: the `.dark` class on <html> switches
// the token sets in app.css, and we re-evaluate when the OS changes while the
// app is open.
const darkMedia = window.matchMedia("(prefers-color-scheme: dark)");
const syncDark = () => document.documentElement.classList.toggle("dark", darkMedia.matches);
syncDark();
darkMedia.addEventListener("change", syncDark);
const target = document.getElementById("app")!;

const app = mount(App, { target });

export default app;
