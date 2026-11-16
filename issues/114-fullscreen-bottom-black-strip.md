Status: ready-for-human

# 修复最大化进入全屏时的底部黑边

## 验收

- 普通窗口、最大化窗口进入全屏时客户区/WebView覆盖整个窗口。
- 退出全屏恢复进入前的最大化/普通状态；进入失败尝试回滚，回滚失败明确报错。
- 侧栏、工作面板和现有全屏交互不回归。

## Comments

- 根因与复现记录见 `.scratch/fullscreen-black-strip-20261116/spec.md`。
- 已修复并通过真实Windows隔离窗口验收：最大化转全屏时客户区从2048×1104正确扩展为2048×1152，黑边48→0；退出恢复最大化，恢复普通窗口正常。
- 真实hook/隔离IPC错误与重复请求回归、视图菜单及原工具栏UI、权限检查和发布构建通过，详见同目录`verification.md`。
- 新EXE位于 `output/CrewVela-fullscreen-fix-20261116-213455/CrewVela.exe`。未提交代码。
