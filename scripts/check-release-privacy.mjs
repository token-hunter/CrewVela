import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const digest = data => createHash('sha256').update(data).digest('hex');

// Inspect compiled files without echoing matched personal paths.
export function inspectReleaseData(data, filename, forbiddenPaths = []) {
  const chunks = [data.toString('utf8'), data.toString('utf16le'), data.subarray(1).toString('utf16le')];
  const findings = [];
  for (const [kind, pattern] of [
    ['windows-user-path', /[a-z]:[\\/]+Users[\\/]+[^\\/\s\x00<>"']+/gi],
    ['mac-user-path', /\/Users\/[^/\s\x00<>"']+/g],
  ]) {
    const values = new Set(chunks.flatMap(text => [...text.matchAll(pattern)].map(match => match[0])));
    if (values.size) findings.push({ kind, count: values.size });
  }
  for (const value of forbiddenPaths) {
    for (const variant of new Set([value, value.replaceAll('\\', '/')])) {
      if (variant && chunks.some(text => text.toLowerCase().includes(variant.toLowerCase()))) {
        findings.push({ kind: 'forbidden-build-root', fingerprint: digest(variant).slice(0, 12) });
      }
    }
  }
  return { file: path.basename(filename), bytes: data.length, sha256: digest(data), findings };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const files = [], forbiddenPaths = [];
  for (let index = 2; index < process.argv.length; index++) {
    const arg = process.argv[index];
    if (arg === '--forbid-path') {
      const value = process.argv[++index];
      if (!value) throw new Error('--forbid-path requires a directory');
      forbiddenPaths.push(value);
    } else if (arg.startsWith('--')) throw new Error(`Unknown option: ${arg}`);
    else files.push(arg);
  }
  if (!files.length) throw new Error('Usage: node scripts/check-release-privacy.mjs <files...> [--forbid-path <directory>]');
  const results = await Promise.all(files.map(async file => inspectReleaseData(await readFile(file), file, forbiddenPaths)));
  process.stdout.write(JSON.stringify(results, null, 2) + '\n');
  if (results.some(result => result.findings.length)) process.exitCode = 1;
}
