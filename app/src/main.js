import { mount } from "svelte";
import "./vendor/dockview.css";
import "./app.css";
import App from "./App.svelte";
import Popout from "./Popout.svelte";
import PanelWindow from "./PanelWindow.svelte";
import SearchWindow from "./SearchWindow.svelte";
import AiChatWindow from "./AiChatWindow.svelte";
import { initSettings } from "./lib/settings.svelte.js";

initSettings();

// the main window, or a reading tab popped out into its own window
const params = new URLSearchParams(location.search);
const popout = params.get("popout");
const panel = params.get("panel");
const searchWin = params.get("search");
const aiChatWin = params.get("aichat");
const target = document.getElementById("app");
if (aiChatWin) {
  mount(AiChatWindow, { target, props: { info: JSON.parse(aiChatWin) } });
} else if (searchWin) {
  mount(SearchWindow, { target, props: { info: JSON.parse(searchWin) } });
} else if (popout) {
  mount(Popout, { target, props: { info: JSON.parse(popout) } });
} else if (panel) {
  mount(PanelWindow, { target, props: { kind: panel } });
} else {
  mount(App, { target });
}
