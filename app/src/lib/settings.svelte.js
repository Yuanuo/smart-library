// User settings: theme, fonts, sizes. Kept in the webview's local storage
// (which lives in the portable data folder) and applied as CSS variables, so
// every window (the main one and the pop-out readers) looks the same; a
// change in one window reaches the others through the `storage` event.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { PLATFORM, MOD_KEY } from "./platform.js";

// Font presets. `default` means "the face the platform itself uses", and that
// differs by OS: Segoe UI + 微软雅黑 on Windows, San Francisco + 苹方 on macOS,
// Cantarell + Noto Sans CJK on Linux. The interface and the reader both start
// there; 宋体 / 明体 is a preset of its own for whoever wants a book serif.
// `app.css` carries a broad stack as the fallback for the moment before this
// module runs.

const DEFAULT_SANS = {
  windows: 'system-ui, "Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", sans-serif',
  mac: 'system-ui, -apple-system, "PingFang SC", "Hiragino Sans GB", sans-serif',
  linux: 'system-ui, "Cantarell", "Noto Sans CJK SC", "Noto Sans CJK TC", "WenQuanYi Micro Hei", sans-serif',
}[PLATFORM];

const DEFAULT_SERIF = {
  windows:
    '"Palatino Linotype", Georgia, "Times New Roman", "Noto Serif CJK TC", "Source Han Serif TC", "MingLiU", "SimSun", serif',
  mac: '"Iowan Old Style", Palatino, "Songti TC", "Songti SC", "Noto Serif CJK TC", "Source Han Serif TC", serif',
  linux:
    '"Noto Serif CJK TC", "Noto Serif CJK SC", "Source Han Serif TC", "Source Han Serif SC", "AR PL UMing TW", "AR PL UMing CN", serif',
}[PLATFORM];

const OS_NAME = { windows: "Windows", mac: "macOS", linux: "Linux" }[PLATFORM];

const SANS = '"Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", "PingFang SC", "Hiragino Sans GB", "Noto Sans CJK SC", "Source Han Sans SC", "WenQuanYi Micro Hei", sans-serif';
const SERIF = DEFAULT_SERIF;
const KAI = '"KaiTi", "Kaiti TC", "STKaiti", "AR PL UKai TW", serif';
const MONO = 'Consolas, "Courier New", "MingLiU", monospace';

// Both defaults are the platform's own sans: 宋体/SimSun is hinted, bitmap-ish
// at interface and body sizes, and the user should not have to go to the
// settings just to make the text look right. 宋体 stays one click away.
export const UI_FONTS = [
  ["default", `系统默认（${OS_NAME}）`, DEFAULT_SANS],
  ["serif", "宋体 / 明体", DEFAULT_SERIF],
  ["kai", "楷体", KAI],
  ["mono", "等宽", MONO],
  ["custom", "自定义…", null],
];

export const READER_FONTS = [
  ["default", `系统默认（${OS_NAME}）`, DEFAULT_SANS],
  ["serif", "宋体 / 明体", DEFAULT_SERIF],
  ["kai", "楷体", KAI],
  ["mono", "等宽", MONO],
  ["custom", "自定义…", null],
];

// Reader text weight. CSS cannot scale a font's weight against a base (there
// is no rem for font-weight), and most CJK fonts only carry a couple of real
// faces, so this is a small table of levels rather than a slider. `stroke` is
// a fake-bold width in em: it thickens text even when the font has no face for
// the requested weight (0 = off). On fonts with only regular and bold the top
// level has no headroom left, so its emphasis adds a stroke of its own.
export const WEIGHT_LEVELS = [
  ["std", "标准"],
  ["mid", "稍粗"],
  ["bold", "加粗"],
];

// Select-to-look-up in the read-only reader. `ctrl` needs Ctrl/Cmd held while
// selecting (so ordinary selection and copy stay quiet); `auto` looks up every
// selection, and Ctrl/Cmd suppresses it (selecting under Ctrl usually means
// copying). `off` turns the feature off. Editors never use it: selecting text
// there is a constant part of editing.
export const SELECT_LOOKUP_MODES = [
  ["ctrl", `按住 ${MOD_KEY} 选字`],
  ["auto", "选中即查"],
  ["off", "关闭"],
];

const WEIGHTS = {
  std: { body: 400, strong: 700, stroke: 0, strongStroke: 0 },
  mid: { body: 500, strong: 800, stroke: 0.012, strongStroke: 0.012 },
  bold: { body: 650, strong: 900, stroke: 0, strongStroke: 0.024 },
};

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

export const AI_PROVIDER_TEMPLATES = [
  { id: "deepseek", label: "DeepSeek", format: "openai", baseUrl: "https://api.deepseek.com/v1", model: "deepseek-chat", needsKey: true },
  { id: "openai", label: "OpenAI", format: "openai", baseUrl: "https://api.openai.com/v1", model: "gpt-4o", needsKey: true },
  { id: "anthropic", label: "Claude", format: "anthropic", baseUrl: "https://api.anthropic.com", model: "claude-sonnet-4-20250514", needsKey: true },
  { id: "qwen", label: "通义千问", format: "openai", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus", needsKey: true },
  { id: "ollama", label: "本地 Ollama", format: "openai", baseUrl: "http://localhost:11434/v1", model: "qwen2.5", needsKey: false },
];

const DEFAULTS = {
  theme: "auto", // auto | light | dark
  uiFont: "default",
  uiFontCustom: "",
  uiZoom: 100, // percent
  readerFont: "default",
  readerFontCustom: "",
  readerSize: 19, // px
  readerLine: 2, // line height
  readerSpacing: 0.04, // em
  readerPara: 0.35, // paragraph spacing, em
  readerWidth: 46, // max width of the reading column, em
  readerFullWidth: false, // stretch the reading column to the visible area
  readerWeight: "std", // std | mid | bold
  readerScript: "orig", // orig: as the text is | simp: shown in simplified characters
  selectLookup: "ctrl", // reader selection opens the dictionary: ctrl | auto | off
  externalLink: "window", // window | ask | browser
  quickLimit: 200, // title quick search: results per batch
  quickPinyin: false, // title quick search: allow matching by pinyin (full or initials)
  dailyOn: true, // the sentence of the day
  dailySources: [], // its sources (search range items); none = every library
  aiOn: true, // AI assistant master switch (visible by default for discoverability)
  aiActiveId: "default-openai", // currently selected model preset id
  aiPresets: [{ ...DEFAULT_AI_PRESET }],
  aiSaveMode: "ask", // ask | default
  aiSaveLib: "", // default target library id
  aiSavePath: "", // default folder/parent path
  aiSnapshotMax: 10, // max temporary AI snapshots per document
  aiSnapshotDays: 7, // retention days for temporary AI snapshots
  shortcuts: {}, // { [actionId]: comboString } overrides
};

function load() {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) || "{}");
    const raw = { ...DEFAULTS, ...parsed };
    if (!raw.shortcuts || typeof raw.shortcuts !== "object" || Array.isArray(raw.shortcuts)) {
      raw.shortcuts = {};
    }
    if (!["window", "ask", "browser"].includes(raw.externalLink)) {
      raw.externalLink = "window";
    }
    if (!WEIGHTS[raw.readerWeight]) {
      raw.readerWeight = "std";
    }
    // "sans" used to be a preset; the default is the platform's sans now
    if (raw.uiFont === "sans") {
      raw.uiFont = "default";
    }
    if (raw.readerFont === "sans") {
      raw.readerFont = "default";
    }
    if (!SELECT_LOOKUP_MODES.some(([v]) => v === raw.selectLookup)) {
      raw.selectLookup = "ctrl";
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

/** True when the given URL points to a local endpoint (which may not need an API key). */
export function isLocalAiUrl(url) {
  const u = (url || "").trim().toLowerCase();
  return /^https?:\/\/(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::\d+)?(\/|$)/.test(u);
}

/** True when the preset has usable credentials (remote endpoints require an API key). */
export function isAiConfigured(preset = activeAiPreset()) {
  if (!preset || !preset.baseUrl?.trim() || !preset.model?.trim()) return false;
  if (isLocalAiUrl(preset.baseUrl)) return true;
  return !!preset.apiKey?.trim();
}

/** True when AI is enabled and the active preset has usable configuration. */
export function isAiReady() {
  if (!settings.aiOn) return false;
  return isAiConfigured(activeAiPreset());
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

function stack(kind, custom, fallback, list) {
  if (kind === "custom") return custom.trim() ? `"${custom.trim().replace(/"/g, "")}", ${fallback}` : null;
  // a font installed on this computer: `sys:<family>`
  if (kind.startsWith("sys:")) return `"${kind.slice(4).replace(/"/g, "")}", ${fallback}`;
  // "default" is this platform's own stack; a value from an older version
  // that no longer exists falls back to nothing (the CSS default in app.css)
  return list.find((f) => f[0] === kind)?.[2] ?? null;
}

let appliedZoom = 100;

/** Put the settings on the page. */
export function apply() {
  const root = document.documentElement;
  if (settings.theme === "auto") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", settings.theme);
  const set = (name, value) => (value == null ? root.style.removeProperty(name) : root.style.setProperty(name, value));
  set("--ui-font", stack(settings.uiFont, settings.uiFontCustom, "sans-serif", UI_FONTS));
  set("--reader-font", stack(settings.readerFont, settings.readerFontCustom, "serif", READER_FONTS));
  set("--reader-size", `${settings.readerSize}px`);
  set("--reader-line", String(settings.readerLine));
  set("--reader-spacing", `${settings.readerSpacing}em`);
  set("--reader-para", `${settings.readerPara}em`);
  set("--reader-max-width", settings.readerFullWidth ? "none" : `${settings.readerWidth}em`);
  const weight = WEIGHTS[settings.readerWeight] ?? WEIGHTS.std;
  set("--reader-weight", String(weight.body));
  set("--reader-strong-weight", String(weight.strong));
  set("--reader-stroke", `${weight.stroke}em`);
  set("--reader-strong-stroke", `${weight.strongStroke}em`);
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
