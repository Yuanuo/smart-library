import { check } from "@tauri-apps/plugin-updater";

export const updateState = $state({ phase: "idle", visible: false, version: "", notes: "", error: "", downloaded: 0, total: 0 });
let update = null;

export async function checkUpdates(manual = false) {
  if (["checking", "downloading", "installing"].includes(updateState.phase)) { if (manual) updateState.visible = true; return; }
  if (update) { await update.close().catch(() => {}); update = null; }
  updateState.phase = "checking";
  updateState.error = "";
  if (manual) updateState.visible = true;
  try {
    update = await check({ timeout: 15000 });
    if (update) {
      updateState.version = update.version;
      updateState.notes = update.body || "";
      updateState.phase = "available";
      updateState.visible = true;
    } else {
      updateState.phase = "current";
    }
  } catch (e) {
    updateState.phase = "error";
    updateState.error = String(e);
  }
}

export async function downloadUpdate() {
  if (!update || updateState.phase !== "available") return;
  updateState.phase = "downloading";
  updateState.downloaded = 0;
  updateState.total = 0;
  try {
    await update.download((event) => {
      if (event.event === "Started") updateState.total = event.data.contentLength || 0;
      if (event.event === "Progress") updateState.downloaded += event.data.chunkLength;
    });
    updateState.phase = "ready";
  } catch (e) {
    updateState.error = String(e);
    updateState.phase = "download-error";
  }
}

export async function installUpdate() {
  if (!update || updateState.phase !== "ready") return;
  updateState.phase = "installing";
  try { await update.install(); }
  catch (e) { updateState.error = String(e); updateState.phase = "install-error"; }
}
