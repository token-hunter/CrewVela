# 首次公开发布检查清单

更新日期：2026-11-09。用于核对源码与安装包的发行条件，不代表所有检查已经通过。`v0.1.0` Release 已创建，仓库可见性单独管理；实际验证结果见[版本说明](releases/v0.1.0.md)。下表保留尚待完成的验收事项。

## 发布判断

发行名称使用 **CrewVela v0.1.0**。当前 package.json、package-lock.json、Cargo.toml/Cargo.lock 和 Tauri 版本保持 0.1.0。补丁使用 0.1.1、新功能使用 0.2.0；达到稳定安装、升级和核心工作流门槛后再规划 1.0.0。日期保留在 changelog 和内部构建记录，发行文件不使用任务名。

## 公开前优先解决

| 项目 | 目前证据 | 完成条件 |
| --- | --- | --- |
| 无源码依赖的安装体验 | 当前源码的 release 已移除源码目录发现；Codex 已安装与 npx 两条路径复用包装桥，固定适配器版本；新 Windows 包已构建，Mac 新包待验证 | 在一次性系统账号/干净 VM 安装 Node 与两个 CLI，安装目录不含源码，不挂载开发机工作区，验证 Claude Chat/Codex Chat、CLI、双向通信；记录下载和失败提示 |
| 核心演示 | 旧工作区存在通信验收；不能直接当作新包验收 | 从同一个候选版本完成“实现→审查→回传修复”，记录双方可见消息、恢复和交接；不用合成响应冒充 |
| Mac 发行验收 | v2 报告有构建副本主窗口与正常退出；安装副本仅启动观察通过 | 从最终 DMG 拖入 Applications，完整启动/发送/退出/重开、旧配置升级及普通/隔离并存测试；按相同源码提交生成 |
| 依赖风险 | 当前源码 npm 全量与生产审计均为 0；历史基线曾有 16 项，旧附件保留在私有备份 | 保留升级回归证据，并继续 Rust 依赖审计；不能把旧附件声称为已修复 |
| 素材再分发 | 根许可 MIT；品牌 logo、吉祥物和第三方改编资源并非全部完成出处核对 | 完成素材清单、许可副本及必要署名；来源不明确的资源不纳入公开发行 |
| 签名和系统信誉 | Windows 未做发行签名；Mac 仅 ad-hoc | 明确签名状态；面向普通用户的 Mac 安装包完成 Developer ID 签名与公证，不关闭系统防护 |
| 私密漏洞渠道 | 仓库已创建，报告入口尚未验证 | 开启并实际检查 GitHub private vulnerability reporting |

历史审计原始 JSON 留在忽略目录中。隐私修复将 MCP SDK 更新至 1.32.2、Vite 更新至 6.4.6、React 插件更新至 4.7.0，并更新兼容的间接依赖；没有使用 audit fix --force。升级后需保留通信、浏览器控制、构建和 UI 回归证据。零告警不等于没有未知漏洞。

## 已准备

- [x] 中文首页、英文 README、详细使用手册独立链接。
- [x] 贡献指南、问题/功能模板、PR 模板、安全政策草稿。
- [x] 版本、资产命名、发布说明草稿与素材核对表。
- [x] 原创工作方式示意图；组件预览的合成数据与来源说明。
- [x] 既有干净 Git 历史和忽略规则；此前公开候选扫描零命中。

## 安装包与源码

Windows 继续运行 `npm run tauri -- build --no-bundle`。输出应用 EXE；先提供便携 ZIP 即可，安装向导为另一个独立产物。Mac 在原生 ARM64 主机运行 `npm run build:macos`，需要的签名配置仅通过环境或 CI Secrets 注入。

发行资产仅来自经过验收的同一源码提交：

- Windows NSIS 安装向导 EXE、应用 EXE及便携 ZIP。
- macOS ARM64 DMG（已接收并统一包名，保留 Mac 原生验收边界）。
- 覆盖每个实际资产的 `SHA256SUMS.txt`。
- 中英安装说明、已知问题和第三方许可。
- GitHub tag 自动提供的源码 ZIP/tar.gz；无需提交 target、node_modules 或个人状态。

首发不承诺 Intel Mac 或 Linux 安装包。先给干净机器试用者完整体验，再扩大平台宣传。

## 发版当天

1. 确认公开的是本开源副本的独立历史，核对作者元数据、整个历史与当前文件的敏感信息扫描。
2. 锁定源码提交和版本，执行测试、构建、安装与卸载/数据保留验收；核对构建版本一致。
3. 对最终资产算 SHA-256；更改任意一个字节都要重算，不混用旧包的散列。
4. 更新 [v0.1.0 说明](releases/v0.1.0.md)，逐平台写实际验证与未验证项，保留既有失败证据。
5. 核对 GitHub 仓库描述、topics、私密报告入口和分享图；创建与版本对应的 Release，上传资产复核后发布。仓库可见性变更单独确认。
6. 在全新用户环境跟随 README 操作，确认下载链接、文件名和版本、图片、教程不依赖开发机。
7. 小范围测试后发演示和文章；只分享真实结果，不刷 Star 或批量骚扰社区。

## 参考

- [SemVer 2.0.0](https://semver.org/)
- [GitHub README](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)
- [GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)
- [Mac 签名与公证](https://v2.tauri.app/distribute/sign/macos/)
