#!/usr/bin/env node
//! Rewrite a tauri updater `latest.json` so asset URLs use the public release
//! download path instead of the api.github.com asset endpoint.
//!
//! tauri-action always writes `https://api.github.com/repos/<owner>/<repo>/releases/assets/<id>`
//! into latest.json. Authenticated through no token, that endpoint shares
//! GitHub's 60-requests-per-hour-per-IP limit, so clients behind a shared
//! address get "403 Forbidden" when downloading an update. The same files are
//! public at `https://github.com/<owner>/<repo>/releases/download/<tag>/<name>`.
//!
//! usage: node tools/fix-updater-json.cjs <latest.json> <assets.tsv> <owner/repo> <tag>
//!
//! `<assets.tsv>` has one `<asset id>\t<asset name>` line per release asset, as
//! produced by `gh api repos/<owner>/<repo>/releases/tags/<tag> --jq ...`.

const fs = require("node:fs");

const [latestPath, assetsPath, repo, tag] = process.argv.slice(2);
if (!latestPath || !assetsPath || !repo || !tag) {
  console.error("usage: fix-updater-json.cjs <latest.json> <assets.tsv> <owner/repo> <tag>");
  process.exit(2);
}

const latest = JSON.parse(fs.readFileSync(latestPath, "utf8"));
const names = new Map();
for (const line of fs.readFileSync(assetsPath, "utf8").split(/\r?\n/)) {
  const tab = line.indexOf("\t");
  if (tab > 0) names.set(line.slice(0, tab).trim(), line.slice(tab + 1).trim());
}

let changed = 0;
for (const entry of Object.values(latest.platforms || {})) {
  const match = /\/releases\/assets\/(\d+)$/.exec(entry.url || "");
  if (!match) continue;
  const name = names.get(match[1]);
  if (!name) {
    console.error(`no asset name for id ${match[1]}`);
    process.exit(1);
  }
  entry.url = `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(name)}`;
  changed += 1;
}

if (changed === 0) {
  console.log("latest.json already uses download URLs");
  process.exit(0);
}
fs.writeFileSync(latestPath, `${JSON.stringify(latest, null, 2)}\n`);
console.log(`rewrote ${changed} platform URLs`);
