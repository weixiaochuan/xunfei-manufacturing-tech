#!/usr/bin/env node
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve("dev-plugins", "packages");

if (!existsSync(root)) {
  console.log("No dev plugin package directory found; skipping plugin package verification.");
  process.exit(0);
}

const packages = readdirSync(root)
  .filter((name) => name.endsWith(".firstwork-plugin"))
  .map((name) => join(root, name));

if (packages.length === 0) {
  console.log("No .firstwork-plugin packages found; skipping plugin package verification.");
  process.exit(0);
}

for (const plugin of packages) {
  const result = spawnSync(
    process.execPath,
    ["scripts/plugin-package.mjs", "verify", plugin],
    { stdio: "inherit" },
  );
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}
