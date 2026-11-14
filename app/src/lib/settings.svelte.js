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
export const DEFAULT_AI_PRESET = {
  id: "default-openai",
  name: "默认模型（OpenAI 兼容）",
  format: "openai", // openai | anthropic
  baseUrl: "https://api.deepseek.com/v1",
  apiKey: "",
  model: "deepseek-chat",
  models: [],
  effortByModel: {},
  systemPrompt: "你是 Smart Library 知识库的 AI 助手，擅长经典文献研读、笔记撰写、创新方案构思、脑图大纲生成与白板绘图。",
};

const DEFAULTS = {
  theme: "auto", // auto | light | dark
  uiFont: "default",
  uiFontCustom: "",
  uiZoom: 100, // percent
  readerFont: "default",
  readerFontCustom: "",
  readerSize: 19, // px
  readerLine: 2, // line height
  readerScript: "orig", // orig: as the text is | simp: shown in simplified characters
  externalLink: "window", // window | ask | browser
  quickLimit: 200, // title quick search: results per batch
  quickPinyin: false, // title quick search: allow matching by pinyin (full or initials)
  dailyOn: true, // the sentence of the day
  dailySources: [], // its sources (search range items); none = every library
  aiOn: false, // AI assistant master switch
  aiActiveId: "default-openai", // currently selected model preset id
  aiPresets: [{ ...DEFAULT_AI_PRESET }],
  aiSaveMode: "ask", // ask | default
  aiSaveLib: "", // default target library id
  aiSavePath: "", // default folder/parent path
};

function load() {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) || "{}");
    const raw = { ...DEFAULTS, ...parsed };
    if (!["window", "ask", "browser"].includes(raw.externalLink)) {
      raw.externalLink = "window";
    }
    if (!Array.isArray(raw.aiPresets) || !raw.aiPresets.length) {
      raw.aiPresets = [{ ...DEFAULT_AI_PRESET }];
    } else {
      raw.aiPresets = raw.aiPresets.map((p) => ({
        ...DEFAULT_AI_PRESET,
        ...p,
        models: Array.isArray(p?.models) ? p.models.filter((m) => typeof m === "string" && m.trim()) : [],
        effortByModel: p?.effortByModel && typeof p.effortByModel === "object" && !Array.isArray(p.effortByModel)
          ? p.effortByModel
          : {},
      }));
    }
    if (!raw.aiPresets.some((p) => p.id === raw.aiActiveId)) {
      raw.aiActiveId = raw.aiPresets[0].id;
    }
    return raw;
  } catch {
    return { ...DEFAULTS, aiPresets: [{ ...DEFAULT_AI_PRESET }] };
  }
}

export const settings = $state(load());

/** The currently selected AI model preset. */
export function activeAiPreset() {
  const list = settings.aiPresets ?? [];
  return list.find((p) => p.id === settings.aiActiveId) ?? list[0] ?? DEFAULT_AI_PRESET;
}

/** True when AI is enabled and the active preset has a URL and model name. */
export function isAiReady() {
  if (!settings.aiOn) return false;
  const p = activeAiPreset();
  return !!(p && p.baseUrl?.trim() && p.model?.trim());
}

/** Convert a preset to the backend `AiModelConfig` struct. */
export function toBackendAiConfig(preset = activeAiPreset()) {
  return {
    format: preset?.format || "openai",
    base_url: (preset?.baseUrl || "").trim(),
    api_key: (preset?.apiKey || "").trim(),
    model: (preset?.model || "").trim(),
  };
}

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
