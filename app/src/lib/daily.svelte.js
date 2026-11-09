// The sentence of the day: once a day (the program may stay open for weeks,
// so it is checked on a timer, not at start-up) a random sentence is drawn
// from the sources set in the settings, when there is an index to draw from.
// `seen` is false until the user has looked at it (the badge on the button).
import * as api from "./api.js";
import { settings } from "./settings.svelte.js";
import { localDate } from "./diary.js";

const KEY = "sl.daily";

function load() {
  try {
    const d = JSON.parse(localStorage.getItem(KEY) || "{}");
    return { item: d.item ?? null, date: d.date ?? "", seen: d.seen ?? true };
  } catch {
    return { item: null, date: "", seen: true };
  }
}

export const daily = $state(load());

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify({ item: daily.item, date: daily.date, seen: daily.seen }));
  } catch {}
}

async function draw() {
  const withins = JSON.parse(JSON.stringify(settings.dailySources || []));
  return api.randomSentence({ lib: null, filters: null, withins: withins.length ? withins : null });
}

/** Draw today's sentence when there is none yet (called on a timer). */
export async function check(libraries) {
  if (!settings.dailyOn || !libraries.some((l) => l.index_ready)) return;
  const today = localDate();
  if (daily.item && daily.date === today) return;
  try {
    daily.item = await draw();
    daily.date = today;
    daily.seen = false;
    save();
  } catch {
    // no index to draw from yet, or the sources are gone: next time
  }
}

/** Another sentence now (it counts as seen). Throws when none can be drawn. */
export async function again() {
  daily.item = await draw();
  daily.date = localDate();
  daily.seen = true;
  save();
  return daily.item;
}

export function markSeen() {
  if (daily.seen) return;
  daily.seen = true;
  save();
}
