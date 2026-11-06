import { mount } from "svelte";
import "./vendor/dockview.css";
import "./app.css";
import App from "./App.svelte";
import Popout from "./Popout.svelte";

// the main window, or a reading tab popped out into its own window
const popout = new URLSearchParams(location.search).get("popout");
const target = document.getElementById("app");
if (popout) {
  mount(Popout, { target, props: { info: JSON.parse(popout) } });
} else {
  mount(App, { target });
}
