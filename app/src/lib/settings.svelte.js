// User settings: theme, fonts, sizes. Kept in the webview's local storage
// (which lives in the portable data folder) and applied as CSS variables, so
// every window (the main one and the pop-out readers) looks the same; a
// change in one window reaches the others through the `storage` event.
import { getCurrentWebview } from "@tauri-apps/api/webview";

export const FONTS = [
  ["default", "宋体 / 明体（默认）", null],
  ["sans", "黑体 / 无衬线", '"Microsoft YaHei", "Noto Sans TC", "Noto Sans SC", "PingFang SC", "PingFang TC", sans-serif'],
  ["kai", "楷体", '"KaiTi", "Kaiti TC", "STKaiti", "AR PL UKai TW", serif'],
  ["mono", "等宽", 'Consolas, "Courier New", "MingLiU", monospace'],
  ["custom", "自定义…", null],
];

const KEY = "sl.settings";
const DEFAULTS = {
  theme: "auto", // auto | light | dark
  uiFont: "default",
  uiFontCustom: "",
  uiZoom: 100, // percent
  readerFont: "default",
  readerFontCustom: "",
  readerSize: 19, // px
  readerLine: 2, // line height
  quickLimit: 200, // title quick search: results per batch
  dailyOn: true, // the sentence of the day
  dailySources: [], // its sources (search range items); none = every library
};

function load() {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) || "{}") };
  } catch {
    return { ...DEFAULTS };
  }
}

export const settings = $state(load());

function stack(kind, custom, fallback) {
  if (kind === "custom") return custom.trim() ? `"${custom.trim().replace(/"/g, "")}", ${fallback}` : null;
  // a font installed on this computer: `sys:<family>`
  if (kind.startsWith("sys:")) return `"${kind.slice(4).replace(/"/g, "")}", ${fallback}`;
  return FONTS.find((f) => f[0] === kind)?.[2] ?? null;
}

let appliedZoom = 100;

/** Put the settings on the page. */
export function apply() {
  const root = document.documentElement;
  if (settings.theme === "auto") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", settings.theme);
  const set = (name, value) => (value == null ? root.style.removeProperty(name) : root.style.setProperty(name, value));
  set("--ui-font", stack(settings.uiFont, settings.uiFontCustom, "sans-serif"));
  set("--reader-font", stack(settings.readerFont, settings.readerFontCustom, "serif"));
  set("--reader-size", `${settings.readerSize}px`);
  set("--reader-line", String(settings.readerLine));
  if (settings.uiZoom !== appliedZoom) {
    appliedZoom = settings.uiZoom;
    getCurrentWebview().setZoom(settings.uiZoom / 100).catch(() => {});
  }
}

export function update(patch) {
  Object.assign(settings, patch);
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...settings }));
  } catch {}
  apply();
}

export function reset() {
  update({ ...DEFAULTS });
}

export function initSettings() {
  apply();
  window.addEventListener("storage", (e) => {
    if (e.key === KEY) {
      Object.assign(settings, load());
      apply();
    }
  });
}
