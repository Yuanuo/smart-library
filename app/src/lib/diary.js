// The diary (design-spec 6.4): dates and times are the user's local ones.
export const DIARY_LIBRARY = "我的日记";

const pad = (n) => String(n).padStart(2, "0");

/** `YYYY-MM-DD` of today (or of `d`). */
export const localDate = (d = new Date()) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

/** `HH:MM` now (or of `d`). */
export const localTime = (d = new Date()) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** The tree path of the month of a date: `我的日记/2026年/11月`. */
export const monthPath = (d = new Date()) => `${DIARY_LIBRARY}/${d.getFullYear()}年/${d.getMonth() + 1}月`;
