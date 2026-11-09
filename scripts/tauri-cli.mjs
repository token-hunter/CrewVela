import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// Keep the public npm build command while removing machine-specific compiler
// source paths. Unit separators preserve paths containing spaces on all hosts.
export function releaseBuildEnvironment(env, projectRoot, homeDirectory) {
  const inherited = env.CARGO_ENCODED_RUSTFLAGS !== undefined
    ? env.CARGO_ENCODED_RUSTFLAGS.split('\x1f').filter(Boolean)
    : (env.RUSTFLAGS ?? '').split(/\s+/).filter(Boolean);
  const mappings = new Map();
  for (const [source, target] of [
    [homeDirectory, '/crewvela/home'],
    [env.CARGO_HOME, '/crewvela/cargo'],
    [env.RUSTUP_HOME, '/crewvela/rustup'],
    [projectRoot, '/crewvela/source'],
  ]) {
    if (!source) continue;
    for (const variant of new Set([source, source.replaceAll('\\', '/')])) {
      mappings.set(variant, target);
      if (/^[A-Za-z]:[\\/]/.test(variant)) mappings.set('\\\\?\\' + variant, target);
    }
  }
  // rustc uses the last applicable mapping; prefer a specific source root.
  const flags = [...inherited, ...[...mappings].sort((a, b) => a[0].length - b[0].length)
    .map(([source, target]) => `--remap-path-prefix=${source}=${target}`)];
  return { ...env, CARGO_ENCODED_RUSTFLAGS: flags.join('\x1f'), CARGO_PROFILE_RELEASE_STRIP: 'debuginfo' };
}

export function isReleaseBuild(args) {
  return args[0] === 'build' && !args.includes('--debug');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (isReleaseBuild(args)) {
    const root = fileURLToPath(new URL('../', import.meta.url)).replace(/[\\/]$/, '');
    const releaseEnv = releaseBuildEnvironment(process.env, root, homedir());
    process.env.CARGO_ENCODED_RUSTFLAGS = releaseEnv.CARGO_ENCODED_RUSTFLAGS;
    process.env.CARGO_PROFILE_RELEASE_STRIP = releaseEnv.CARGO_PROFILE_RELEASE_STRIP;
  }
  const require = createRequire(import.meta.url);
  const cli = require('@tauri-apps/cli');
  await cli.run(args, `npm run ${process.env.npm_lifecycle_event ?? 'tauri'}`);
}
