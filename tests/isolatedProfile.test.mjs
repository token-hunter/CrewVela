import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,mkdtempSync,writeFileSync,rmSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import path from 'node:path';
const root=new URL('../',import.meta.url);
test('isolated launcher opts in with a separate ignored directory and never mutates the real profile',()=>{
  const launcher=readFileSync(new URL('scripts/start-isolated-test.ps1',root),'utf8');
  assert.match(launcher,/--test-data-dir/);assert.match(launcher,/'\.test-data'/);
  assert.doesNotMatch(launcher,/Remove-Item|SetEnvironmentVariable|Move-Item|Copy-Item/);
  const shortcut=readFileSync(new URL('start-first-run-test.cmd',root),'utf8');
  assert.match(shortcut,/--test-data-dir "%~dp0\.test-data"/);
  const temp=mkdtempSync(path.join(tmpdir(),'crewvela-isolated-ignore-'));
  try{
    writeFileSync(path.join(temp,'.gitignore'),readFileSync(new URL('.gitignore',root)));
    execFileSync('git',['init','--quiet',temp]);
    const ignored=execFileSync('git',['check-ignore','--stdin'],{cwd:temp,input:'.test-data/config/sessions.json\n.test-data/webview/cache\n',encoding:'utf8'});
    assert.deepEqual(ignored.trim().split(/\r?\n/),['.test-data/config/sessions.json','.test-data/webview/cache']);
  }finally{rmSync(temp,{recursive:true,force:true});}
});
