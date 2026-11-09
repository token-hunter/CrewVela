import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const retiredName = /c{2}[ _-]*(?:dock|launcher)|c{2}launcher/i;
// These exact compatibility declarations are required to read released data.
// The migration report quotes old identities as evidence, not active branding.
const compatibilityLine = {
  'src-tauri/src/brand.rs': /^pub\(crate\) const LEGACY_/,
  'scripts/legacy-brand.mjs': /^export const LEGACY_NAME/,
  'scripts/ai-memory-project-hook.ps1': /^\s*\$legacyConfigRoot =/,
};
const directories = ["src", "src-tauri/src", "src-tauri/resources", "src-tauri/examples", "scripts", "tools", "skills", "tests", "docs", "issues"];
const rootFiles = ["README.md", "CLAUDE.md", "AGENTS.md", ".gitignore", ".env.example", "index.html", "pet-preview.html", "package.json", "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json"];
const textExtensions = new Set([".rs", ".ts", ".tsx", ".mjs", ".cjs", ".js", ".css", ".md", ".json", ".html", ".ps1", ".toml", ".svg", ".xml"]);

function files(directory) {
  if (!existsSync(path.join(root, directory))) return [];
  return readdirSync(path.join(root, directory), { withFileTypes: true }).flatMap((entry) => {
    const name = `${directory}/${entry.name}`;
    if (entry.isSymbolicLink()) return [];
    return entry.isDirectory() ? files(name) : [name];
  });
}

test("current product source, documentation, protocols and filenames use the CrewVela identity", () => {
  for (const required of ['src', 'src-tauri/src', 'skills', 'scripts', 'tests']) {
    assert.ok(existsSync(path.join(root, required)), `Required source tree is missing: ${required}`);
  }
  const paths = [...rootFiles, ...directories.flatMap(files)].filter((name) => existsSync(path.join(root, name)));
  const matches = [];
  for (const name of paths) {
    if (retiredName.test(name)) matches.push(`${name} (filename)`);
    if (!rootFiles.includes(name) && !textExtensions.has(path.extname(name))) continue;
    if (name === 'docs/legacy-brand-audit.md') continue;
    for (const [index, line] of readFileSync(path.join(root, name), "utf8").split(/\r?\n/).entries()) {
      if (retiredName.test(line) && !compatibilityLine[name]?.test(line)) matches.push(`${name}:${index + 1}`);
    }
  }
  assert.deepEqual(matches, [], "Retired product identifiers must not return to current source or documentation");
});
