import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import path from "node:path";

export const memoryVersion = "3.5.1";
const packages = {
  "win32:x64": ["ai-memory-windows-x86_64.zip", 28010147, "867da04c121910b019d4525c7afcdbd300eefebf310b3f5112bd19a043c211ef"],
  "darwin:arm64": ["ai-memory-macos-aarch64.tar.gz", 22757915, "fda158379dc03331e5c71ab6b702a9335c7fc41e75dda4a2b1a7d1e8e98aaf04"],
  "darwin:x64": ["ai-memory-macos-x86_64.tar.gz", 24512029, "49ff09af2eb904abf077a4a2a87660cad7800c6d6f4843abcab487f9315577c9"],
};

export function memoryBuildPackage(args, platform = process.platform, arch = process.arch) {
  const index = args.indexOf("--target");
  const target = index >= 0 ? args[index + 1] : args.find(arg => arg.startsWith("--target="))?.slice(9);
  if (target) {
    if (target.includes("windows")) return target.startsWith("x86_64-") ? packages["win32:x64"] : undefined;
    if (target.includes("apple-darwin")) {
      if (target.startsWith("aarch64-")) return packages["darwin:arm64"];
      if (target.startsWith("x86_64-")) return packages["darwin:x64"];
    }
    return undefined;
  }
  return packages[`${platform}:${arch}`];
}

export async function prepareEmbeddedMemory(root, args) {
  const asset = memoryBuildPackage(args);
  if (!asset) return undefined;
  const [name, size, sha256] = asset;
  const cache = path.join(root, "src-tauri", "target", "crewvela-memory-assets");
  const archive = path.join(cache, `${memoryVersion}-${name}`);
  const matches = bytes => bytes.length === size && createHash("sha256").update(bytes).digest("hex") === sha256;
  await mkdir(cache, { recursive: true });
  const cached = await readFile(archive).catch(error => {
    if (error.code !== "ENOENT") throw error;
  });
  if (cached && matches(cached)) return archive;

  const source = `https://github.com/akitaonrails/ai-memory/releases/download/v${memoryVersion}/${name}`;
  const response = await fetch(source, { signal: AbortSignal.timeout(180_000) });
  if (!response.ok || !response.body) throw Error(`内置共享记忆下载失败（HTTP ${response.status}）`);
  const chunks = [];
  let received = 0;
  for await (const chunk of response.body) {
    received += chunk.length;
    if (received > size) throw Error("内置共享记忆安装包超过预期大小");
    chunks.push(chunk);
  }
  const bytes = Buffer.concat(chunks, received);
  if (!matches(bytes)) throw Error("内置共享记忆安装包 SHA256 或大小不符");
  const temporary = path.join(cache, `.${randomUUID()}.tmp`);
  try {
    await writeFile(temporary, bytes, { flag: "wx" });
    await rename(temporary, archive);
  } finally {
    await rm(temporary, { force: true });
  }
  return archive;
}
