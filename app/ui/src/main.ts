import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import Mini from "./lib/Mini.svelte";
import { bindTheme } from "./lib/prefs.svelte";

// The floating overlay is the same bundle, loaded at index.html#mini.
const overlay = location.hash === "#mini";
if (overlay) document.documentElement.classList.add("mini-view");
bindTheme(overlay);

mount(overlay ? Mini : App, { target: document.getElementById("app")! });
