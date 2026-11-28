import { readFile, stat, writeFile } from "node:fs/promises";
import { basename, dirname, join } from "node:path";

const [tag, installer] = process.argv.slice(2);
if (!tag || !installer || !/^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-beta(?:\.(?:0|[1-9]\d*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*))*)?(?:\+[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*)?$/.test(tag)) {
  throw Error("Usage: node scripts/make-windows-update-manifest.mjs <version tag> <x64 NSIS setup.exe>");
}

const name = basename(installer);
if (!/^CrewVela[-_][A-Za-z0-9.+_-]+[-_]x64-setup\.exe$/i.test(name)) {
  throw Error("Expected a CrewVela x64 NSIS installer filename");
}
const { size } = await stat(installer);
if (size === 0 || size > 512 * 1024 * 1024) throw Error("Unexpected installer size");

const signature = (await readFile(installer + ".sig", "utf8")).trim();
if (!/^[a-zA-Z0-9+/]+={0,2}$/.test(signature)) throw Error("Expected a Tauri updater signature next to the installer");

const output = join(dirname(installer), "crewvela-update.json");
const manifest = {
  version: tag.replace(/^v/, ""),
  platforms: {
    "windows-x86_64": {
      url: `https://github.com/token-hunter/CrewVela/releases/download/${tag}/${name}`,
      signature,
    },
  },
};
await writeFile(output, JSON.stringify(manifest, null, 2) + "\n");
console.log(output);
