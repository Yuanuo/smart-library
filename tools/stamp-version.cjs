// Stamp the version as today's local date in `YY.MM.DD` form (semver spells
// it `26.11.18`). Runs before every `npm run tauri` command so a build always
// carries the day it was made. Packages and the installer read the version at
// startup, so they must be rewritten before the CLI loads them.
const fs = require("node:fs");
const path = require("node:path");

// Always work from the repository root, whatever directory npm was run from.
const root = path.resolve(__dirname, "..");
const at = (p) => path.join(root, p);

const date = new Date();
const version = [
  String(date.getFullYear()).slice(-2),
  date.getMonth() + 1,
  date.getDate(),
].join(".");

const read = (p) => fs.readFileSync(at(p), "utf8");
const write = (p, text) => fs.writeFileSync(at(p), text);

function replaceOnce(file, pattern, replacement) {
  const text = read(file);
  if (!pattern.test(text)) throw new Error(`Cannot find version in ${file}`);
  const next = text.replace(pattern, replacement);
  if (next !== text) write(file, next);
}

// JSON files: keep the formatting npm and Tauri expect.
for (const file of ["app/package.json", "app/package-lock.json", "app/src-tauri/tauri.conf.json"]) {
  const json = JSON.parse(read(file));
  if (json.lockfileVersion) {
    json.version = version;
    if (json.packages && json.packages[""]) json.packages[""].version = version;
  } else {
    json.version = version;
  }
  write(file, JSON.stringify(json, null, 2) + "\n");
}

// Cargo manifests: the first `version = "..."` is the package version.
for (const file of ["core/Cargo.toml", "cli/Cargo.toml", "app/src-tauri/Cargo.toml"]) {
  replaceOnce(file, /^version\s*=\s*"[^"]*"/m, `version = "${version}"`);
}

// Cargo.lock: only the three workspace packages.
{
  const text = read("Cargo.lock");
  const next = text.replace(
    /(name = "smart-library-(?:app|cli|core)"\nversion = ")[^"]*"/g,
    `$1${version}"`,
  );
  if (next !== text) write("Cargo.lock", next);
}

console.log(`version ${version}`);
