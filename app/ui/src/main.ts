import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import Mini from "./lib/Mini.svelte";
import Phone from "./lib/mobile/Phone.svelte";
import { isPhone } from "./lib/platform";
import { bindTheme } from "./lib/prefs.svelte";

// The floating overlay is the same bundle, loaded at index.html#mini.
const overlay = location.hash === "#mini";
if (overlay) document.documentElement.classList.add("mini-view");
bindTheme(overlay);

// The phone app is the same bundle with its own screens (ADR-015 Decision 6).
const phone = !overlay && isPhone();
if (phone) document.documentElement.classList.add("phone-view");
mount(overlay ? Mini : phone ? Phone : App, { target: document.getElementById("app")! });
