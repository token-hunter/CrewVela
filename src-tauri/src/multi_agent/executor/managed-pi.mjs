import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createEditToolDefinition, createWriteToolDefinition } from '@earendil-works/pi-coding-agent';

// Use the native Pi tools and their matching, diffing, cancellation and queue semantics.
// Their filesystem operations must pass the live run's authority in CrewVela.
export default async function crewvelaManagedPi(pi) {
  const { default: registerResearch } = await import(pathToFileURL(process.env.CREWVELA_PI_RESEARCH_EXTENSION).href);
  const root = process.cwd();
  await registerResearch(pi, rpc => {
    async function call(name, args) {
      const result = await rpc('tools/call', { name, arguments: args });
      if (result.isError) throw new Error(result.content?.[0]?.text ?? 'File operation rejected');
      return result.structuredContent;
    }
    const relative = file => {
      const name = path.relative(root, file);
      if (!name || path.isAbsolute(name) || name === '..' || name.startsWith('..' + path.sep)) throw new Error('File outside the task worktree');
      return name.split(path.sep).join('/');
    };
    const write = createWriteToolDefinition(root, { operations: {
      // The authorized write creates its parent after checking the complete destination.
      mkdir: async () => {},
      writeFile: (file, text) => call('cc_pi_write_file', { path: relative(file), text }),
    } });
    const revisions = new Map();
    const edit = createEditToolDefinition(root, { operations: {
      access: async file => { relative(file); },
      readFile: async file => {
        const result = await call('cc_pi_edit_read', { path: relative(file) });
        revisions.set(file, result.revision);
        return Buffer.from(result.text, 'utf8');
      },
      writeFile: async (file, text) => {
        const revision = revisions.get(file);
        revisions.delete(file);
        if (!revision) throw new Error('Missing edit source revision');
        await call('cc_pi_write_file', { path: relative(file), text, revision });
      },
    } });
    // Unique names keep the built-in unrestricted tools disabled if this extension fails to load.
    pi.registerTool({ ...write, name: 'crewvela_write' });
    pi.registerTool({ ...edit, name: 'crewvela_edit' });
    return ['crewvela_write', 'crewvela_edit'];
  });
}
