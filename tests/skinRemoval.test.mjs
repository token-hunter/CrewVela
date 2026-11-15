import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
const read = path => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

test("皮肤入口、渲染层、配置模块和后端命令整条移除", () => {
  for (const path of ["src/components/SkinSettingsPanel.tsx", "src/components/SkinAtmosphere.tsx", "src/components/DynamicSkinLayer.tsx", "src/customSkins.ts", "src/deepSkins.ts", "src/skinIds.ts", "src/companionSettings.ts", "src-tauri/src/dynamic_skins.rs"]) {
    assert.equal(existsSync(new URL(`../${path}`, import.meta.url)), false, path);
  }
  const source = read("src/App.tsx") + read("src/components/SettingsDialog.tsx") + read("src-tauri/src/lib.rs");
  assert.doesNotMatch(source, /SkinSettingsPanel|SkinAtmosphere|DynamicSkinLayer|companionSettings|import_dynamic_skin|delete_dynamic_skin|attach_dynamic_skin_audio|read_skin_config_file|dynamic_skins/);
});

test("外观透明度、独立桌宠及文件预览共享能力保留", () => {
  const transparency = read("src/styles/appearance-transparency.css");
  assert.match(read("src/styles.css"), /appearance-transparency\.css/);
  assert.match(transparency, /data-global-transparent="true"/);
  assert.match(transparency, /data-terminal-transparent="true"/);
  assert.match(read("src/App.tsx"), /<DesktopPet/);
  assert.match(read("src/components/SettingsDialog.tsx"), /<PetSettingsPanel/);
  const protocol = JSON.parse(read("src-tauri/tauri.conf.json")).app.security.assetProtocol;
  assert.equal(protocol.enable, true);
  assert.equal(protocol.scope.some(path => path.includes("skins")), false);
  assert.match(read("src/components/FileExplorerPanel.tsx"), /convertFileSrc/);
});
