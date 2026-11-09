Status: ready-for-human

# 制作 Windows 安装向导并统一双平台包名

- 接收用户提供的 Mac DMG，与 v2 报告散列比对，原文件不改。
- Windows 保留指定的 no-bundle 编译命令，再单独生成 NSIS setup EXE；仅新增平台配置。
- 为安装包准备当前用户安装、中英界面和许可资源，不在正式用户环境做安装/卸载。
- 发行目录统一命名，输出 SHA-256 和实际验证记录，更新已有下载草稿。
- 保留开源准备的未提交文档；不提交、不推送、不发布。

## 实际结果

- 指定 no-bundle 应用构建、NSIS bundle 均成功；新增 `bundle:windows` 独立命令和 Windows 专属配置。
- Node 全量 425 通过、1 项既有跳过，发布资源 1/1 通过。
- 用户 Mac DMG 原件不改，复制件 SHA-256 与 v2 报告完全一致。
- 两个平台统一文件名，SHA256SUMS 覆盖最终副本；Windows 向导实际检查欢迎页和许可页后取消，未安装/卸载。
- Windows 未签名；Mac 沿用 ad-hoc 未公证；干净机器业务、Mac 安装副本正常退出等验收边界保留。
- 产物位于忽略的 `output/releases/v0.1.0/`，不进入 Git；未提交、未推送、未发布。

## Comments

- 2026-11-09：用户后续授权提交安装包配置、npm 命令及配套发布文档，不推送；实际 EXE/DMG 继续留在忽略目录，留作后续 Release 附件。
