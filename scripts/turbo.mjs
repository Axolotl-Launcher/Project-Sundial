// Runs a turbo task, scoped to the platform shell of the current OS.
//
// turbo has no notion of the host OS, so `turbo run build` would otherwise
// build every platform shell — compiling the same Rust code once per shell
// for a binary only one of them can use.
import { spawnSync } from "node:child_process";

const PLATFORM_PACKAGES = {
  win32: "@superlogical/platform-windows",
  linux: "@superlogical/platform-linux",
};

const [task, ...rest] = process.argv.slice(2);
if (!task) {
  console.error("usage: node scripts/turbo.mjs <task> [...turbo args]");
  process.exit(1);
}

const filters = ["--filter=./apps/*"];
const platformPackage = PLATFORM_PACKAGES[process.platform];
if (platformPackage) {
  filters.push(`--filter=${platformPackage}`);
}

const result = spawnSync(
  "turbo",
  ["run", task, ...filters, ...rest],
  { stdio: "inherit", shell: process.platform === "win32" },
);

process.exit(result.status ?? 1);
