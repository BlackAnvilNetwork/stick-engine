#!/usr/bin/env node
// Copyright (c) 2026 Black Anvil Network
// SPDX-License-Identifier: AGPL-3.0-or-later
// Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.
//
// Adds the Black Anvil Network AGPL header to source files (idempotent).
//   node scripts/add-license-headers.mjs          add missing headers
//   node scripts/add-license-headers.mjs --check  exit 1 if any header is missing
// Files are taken from `git ls-files` under ROOTS. Skipped: lockfiles, JSON,
// binaries/images, HTML, YAML, Markdown, generated output (pkg/, target/, dist/).
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const ROOTS = ["."];
const SLASH = new Set(["rs", "js", "mjs", "ts", "wgsl", "glsl", "ron"]);
const HASH = new Set(["toml", "sh", "py"]);
const SKIP_DIR = /(^|\/)(pkg|target|dist|node_modules|vendor)\//;
const LINES = [
  "Copyright (c) 2026 Black Anvil Network",
  "SPDX-License-Identifier: AGPL-3.0-or-later",
  "Licensed under the GNU Affero General Public License v3.0 or later. See LICENSE.",
];

const check = process.argv.includes("--check");
const files = execFileSync("git", ["ls-files", "--", ...ROOTS], { encoding: "utf8" }).split("\n").filter(Boolean);
const missing = [];
let added = 0;
for (const f of files) {
  if (SKIP_DIR.test(f)) continue;
  const ext = f.split(".").pop();
  const c = SLASH.has(ext) ? "//" : HASH.has(ext) ? "#" : null;
  if (!c) continue;
  const src = readFileSync(f, "utf8");
  if (src.includes("SPDX-License-Identifier: AGPL-3.0-or-later")) continue;
  if (check) {
    missing.push(f);
    continue;
  }
  const header = LINES.map((l) => `${c} ${l}`).join("\n") + "\n";
  // A shebang must stay on line 1; everything else (incl. Rust `//!` docs and `#![..]`) may follow comments.
  const out = src.startsWith("#!") && !src.startsWith("#![")
    ? src.replace(/^(#![^\n]*\n)/, `$1${header}`)
    : header + (src.startsWith("\n") ? "" : "\n") + src;
  writeFileSync(f, out);
  added++;
}
if (check) {
  if (missing.length) {
    console.error(`missing license header in ${missing.length} file(s):\n  ${missing.join("\n  ")}\nrun: node scripts/add-license-headers.mjs`);
    process.exit(1);
  }
  console.log("license headers ok");
} else console.log(`added headers to ${added} file(s)`);
