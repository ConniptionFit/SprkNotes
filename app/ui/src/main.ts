import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import Mini from "./lib/Mini.svelte";

// The floating mini controller is the same bundle, loaded at index.html#mini.
const mini = location.hash === "#mini";
if (mini) document.documentElement.classList.add("mini-view");

mount(mini ? Mini : App, { target: document.getElementById("app")! });
