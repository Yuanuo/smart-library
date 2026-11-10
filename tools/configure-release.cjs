// Used only on GitHub Actions. Never writes a private signing key to the repo.
const fs = require("node:fs");
const path = "app/src-tauri/tauri.conf.json";
const config = JSON.parse(fs.readFileSync(path, "utf8"));
const tag = process.env.GITHUB_REF_NAME;
const repo = process.env.GITHUB_REPOSITORY;
const pubkey = process.env.UPDATER_PUBLIC_KEY;
if (!repo || !/^[^/]+\/[^/]+$/.test(repo) || !pubkey) throw new Error("Missing updater repository or public key");
const packageVersion = JSON.parse(fs.readFileSync("app/package.json", "utf8")).version;
const cargoVersion = fs.readFileSync("app/src-tauri/Cargo.toml", "utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (config.version !== packageVersion || config.version !== cargoVersion) throw new Error("Application version files do not match");
if (process.env.GITHUB_REF_TYPE !== "tag" || tag !== `v${config.version}`) throw new Error(`Push tag v${config.version} to release this version`);
config.bundle.createUpdaterArtifacts = true;
config.plugins = { ...config.plugins, updater: { pubkey, endpoints: [`https://github.com/${repo}/releases/latest/download/latest.json`] } };
fs.writeFileSync(path, JSON.stringify(config, null, 2) + "\n");
