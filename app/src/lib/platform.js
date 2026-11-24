// Platform detection and OS-specific UI helpers (paths, modifier keys).

export const PLATFORM = (() => {
  const ua = typeof navigator === "undefined" ? "" : `${navigator.platform || ""} ${navigator.userAgent || ""}`;
  if (/Mac|iPhone|iPad/i.test(ua)) return "mac";
  if (/Linux|X11/i.test(ua) && !/Android/i.test(ua)) return "linux";
  return "windows";
})();

export const IS_MAC = PLATFORM === "mac";
export const IS_WIN = PLATFORM === "windows";
export const MOD_KEY = IS_MAC ? "Cmd" : "Ctrl";

/** Example file path matching the current OS (for input placeholders). */
export const examplePath = (name) => (IS_WIN ? `C:\\…\\${name}` : `~/…/${name}`);
