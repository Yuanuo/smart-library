// Runs the Tauri CLI for `npm run tauri <command>`.
//
// The version is stamped with today's date only for `build`: installers and
// release artifacts carry the day they were made, while `dev` and the other
// commands leave the version files alone so they don't churn the working tree.
const { spawnSync } = require("node:child_process");
const path = require("node:path");

const appDir = path.resolve(__dirname, "..", "app");
const args = process.argv.slice(2);

if (args[0] === "build") {
  const stamp = spawnSync(process.execPath, [path.join(__dirname, "stamp-version.cjs")], {
    stdio: "inherit",
  });
  if (stamp.status !== 0) process.exit(stamp.status ?? 1);
}

const cli = require.resolve("@tauri-apps/cli/tauri.js", { paths: [appDir] });
const run = spawnSync(process.execPath, [cli, ...args], { stdio: "inherit", cwd: appDir });
process.exit(run.status ?? 1);
