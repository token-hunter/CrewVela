import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const panel = readFileSync(new URL("../src/components/FileExplorerPanel.tsx", import.meta.url), "utf8");
const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");

test("项目文件页签接入工作区文件树和受限读取命令", () => {
  assert.match(app, /sidebarTabKind\(tab\) === "files"/);
  assert.match(app, /<MemoFileExplorerPanel/);
  assert.match(app, /root=\{activeWorkspaceRoot\}/);
  assert.match(panel, /workspace_list_directory/);
  assert.match(panel, /workspace_read_text/);
  assert.match(panel, /workspace_search_files/);
  assert.match(panel, /workspace_media_path/);
});

test("预览器覆盖安全 HTML 与原生视频播放", () => {
  assert.match(panel, /sandbox=""/);
  assert.match(panel, /<video src=\{mediaUrl\} controls playsInline preload="metadata"/);
  assert.match(panel, /IMAGE_EXTENSIONS/);
  assert.match(panel, /VIDEO_EXTENSIONS/);
  assert.match(panel, /FilePreviewSettings/);
});

test("文件预览采用 Codex 式左预览右文件树布局并支持调整侧栏宽度", () => {
  assert.ok(
    panel.indexOf('className="file-preview-pane"') < panel.indexOf('className="file-explorer-browser"'),
    "文件内容应位于文件树左侧",
  );
  assert.match(app, /right-sidebar-resizer/);
  assert.match(app, /left-sidebar-resizer/);
  assert.match(panel, /file-preview-divider/);
  assert.match(app, /rightSidebarDesiredWidth, setRightSidebarDesiredWidth/);
  assert.match(app, /crewvela-right-sidebar-width/);
  assert.match(app, /leftSidebarWidth/);
  assert.match(panel, /fileTreeWidth/);
  assert.match(app, /--right-sidebar-width/);
  assert.match(app, /--left-sidebar-width/);
  assert.match(panel, /--file-tree-width/);
  assert.match(app, /onDoubleClick/);
  assert.match(panel, /onDoubleClick/);
  assert.doesNotMatch(app, /Math\.min\(1100/);
  assert.doesNotMatch(app, /window\.innerWidth - 360/);
  assert.doesNotMatch(app, /Math\.min\(520/);
  assert.doesNotMatch(panel, /Math\.min\(480/);
  assert.doesNotMatch(panel, /panelWidth - 320/);
});

test("预览面板支持文本编辑保存与打开所在目录", () => {
  assert.match(panel, /workspace_write_text/);
  assert.match(panel, /workspace_reveal_path/);
  assert.match(panel, /EDITABLE_KINDS/);
  assert.match(panel, /file-preview-editor/);
  assert.match(panel, /file-preview-edit-bar/);
  assert.match(panel, /未保存/);
  // 保存后强制重载 HTML 预览。
  assert.match(panel, /key=\{mediaVersion\}/);
  assert.match(panel, /setMediaVersion/);
});

test("进入编辑前重新读取所选文件，读取失败不能进入编辑", () => {
  const enterEdit = panel.split("const enterEdit = async () => {")[1].split("const cancelEdit =")[0];
  assert.match(enterEdit, /const text = await invoke<string>\("workspace_read_text"/);
  assert.match(enterEdit, /setDraft\(text\);\s*setEditing\(true\)/);
  assert.doesNotMatch(enterEdit, /setDraft\(content\)/);
});

test("关闭文件只清空当前预览，不关闭侧栏或清空文件树", () => {
  const closePreview = panel.split("const closePreview = () => {")[1].split("const enterEdit =")[0];
  assert.match(panel, /aria-label="关闭文件" title="关闭文件"[^>]*onClick=\{closePreview\} disabled=\{saving\}/);
  assert.match(closePreview, /setSelected\(null\)/);
  assert.match(closePreview, /setContent\(""\)/);
  assert.match(closePreview, /setMediaUrl\(""\)/);
  assert.match(closePreview, /setCodeQuery\(""\)/);
  assert.match(closePreview, /setZoom\(1\)/);
  assert.match(closePreview, /setEditing\(false\)/);
  assert.match(closePreview, /setDraft\(""\)/);
  assert.doesNotMatch(closePreview, /setChildren|setExpanded|setQuery|setSearchResults/);
});

test("关闭文件保护未保存草稿并阻止关闭后的文件读取结果复现", () => {
  const closePreview = panel.split("const closePreview = () => {")[1].split("const enterEdit =")[0];
  const enterEdit = panel.split("const enterEdit = async () => {")[1].split("const cancelEdit =")[0];
  assert.match(closePreview, /if \(saving\) return/);
  assert.match(closePreview, /editing && draft !== content && !window\.confirm\(/);
  assert.match(closePreview, /\+\+previewLoadRef\.current/);
  assert.match(enterEdit, /loadId === previewLoadRef\.current/);
  assert.match(panel, /!cancelled && loadId === previewLoadRef\.current/);
});

test("文件树自动加载每个 root 只尝试一次，刷新按钮仍可重试", () => {
  const effects = panel.split("const loadDirectory = useCallback")[1].split("const visibleChildren =")[0];
  assert.match(effects, /if \(lastRootRef\.current !== root\) \{\s*lastRootRef\.current = root;\s*autoLoadedRootRef\.current = null;\s*\}[\s\S]*?if \(root && active && autoLoadedRootRef\.current !== root\) \{\s*autoLoadedRootRef\.current = root;\s*void loadDirectory\("", true\)/);
  assert.match(effects, /autoLoadedRootRef\.current !== root && !children\[""\] && !loading\.has\(""\)\) \{\s*autoLoadedRootRef\.current = root;\s*void loadDirectory\(""\)/);
  assert.match(panel, /aria-label="刷新文件树"[^>]*onClick=\{\(\) => void loadDirectory\("", true\)\}/);
});
