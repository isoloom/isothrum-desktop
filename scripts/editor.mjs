#!/usr/bin/env node
// Puts the editor the app bundles into ../editor, from the @isothrum/editor package.
// The package ships a prebuilt offline bundle (dist/desktop); we copy it. If that bundle is
// missing (a source checkout that hasn't been built), we build it from the package instead.
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const here = resolve(import.meta.dirname, "..");
const out = join(here, "editor");

const require = createRequire(import.meta.url);
const pkgRoot = dirname(require.resolve("@isothrum/editor/package.json"));
const prebuilt = join(pkgRoot, "dist", "desktop");

rmSync(out, { recursive: true, force: true });
if (existsSync(prebuilt)) {
  mkdirSync(out, { recursive: true });
  cpSync(prebuilt, out, { recursive: true });
  console.log(`editor: copied ${prebuilt} -> ${out}`);
} else {
  execFileSync(process.execPath, [join(pkgRoot, "scripts/build-desktop-editor.mjs"), out], { stdio: "inherit" });
}
