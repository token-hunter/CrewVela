import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {trustedReleaseUrl,APP_REPOSITORY_URL,APP_RELEASES_URL} from '../src/appHelp.ts';
test('update actions accept only a validated tag of the configured repository',()=>{
 for(const tag of ['v1.2.3','1.2.3+build.1','v0.1.1-beta.20261128','v1.2.3-beta.1+build.1'])assert.equal(trustedReleaseUrl(`${APP_RELEASES_URL}/tag/${tag}`),true,tag);
 for(const url of ['javascript:alert(1)','file:///tmp/test','https://evil.invalid/v1.2.3',`${APP_RELEASES_URL}/tag/v1.2.3?redirect=bad`,`${APP_RELEASES_URL}/tag/v1.2.3#x`,`${APP_RELEASES_URL}/tag/nested/v1.2.3`,`${APP_RELEASES_URL}/tag/v1.2.3-rc.1`,`${APP_RELEASES_URL}/tag/v1.2.3-beta.01`,`${APP_RELEASES_URL}/tag/v1.2.3-beta.1%2Ffile.exe`,'https://user:pass@github.com/token-hunter/CrewVela/releases/tag/v1.2.3'])assert.equal(trustedReleaseUrl(url),false,url);
});
test('backend and about links use the same project and privacy stays bundled',()=>{
 const backend=readFileSync('src-tauri/src/app_help.rs','utf8');assert.ok(backend.includes(`const REPOSITORY: &str = "${APP_REPOSITORY_URL}"`));
 const privacy=readFileSync('docs/privacy.md','utf8');for(const scope of ['CLI','记忆服务','手动检查应用更新','分别管理'])assert.ok(privacy.includes(scope));
 assert.doesNotMatch(privacy,/所有数据绝不离开本机|保证.*加密|GDPR.*合规/);
});
