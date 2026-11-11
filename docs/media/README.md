# README 媒体与拍摄记录

- `author-drumstick.gif`：按用户要求收录此前生成的「给作者加个鸡腿」搞怪表情包，原样复制；314×314，16 帧，约 2.67 秒无限循环。用于 README 趣味互动区，不是收款码或产品演示。

- `crewvela-collaboration-banner.png`：2026-11-09 用 Image Gen 重新生成的宣传图。保留 CrewVela 橙色球形形象与聚光灯舞台；其余为抽象通用角色，替换原先参考第三方 Agent 角色的部分。不是产品截图或消息收发证据。
  2026-11-11 用内置 ImageGen 编辑标题为 `CrewVela`，移除旧中文副标题，保留舞台、角色与其余文案。
- `desktop-multi-agent-workspace-redacted.png`：用户提供的 Windows 多 Agent CLI 分屏实拍，2026-11-11 更新为用户提供的 1791×964 九宫格截图；按用户选择，用本机脚本定点遮挡本机路径、用户名和历史任务文字，遮挡区域以外像素保持不变，输出不携带图片元数据。
- `desktop-native-chat-redacted.png`：用户提供的 Windows 原生聊天与 Agent 标签实拍，2558×1369；采用相同的定点遮挡方式，保留其余真实像素。两张图均保留拍摄时的图标和状态，外观可能与当前版本不同。
- `crewvela-overview.svg`：项目文字和几何图形组成的工作方式示意，不是产品截图。
- `social-preview.jpg`：上述 SVG 转出的分享图片；本文件存在不代表已设置 GitHub 社交预览。

包含私人目录、历史任务文字或本机测试目录横幅的未脱敏原图，未纳入当前发布树；本页所列两张展示图使用定点遮挡后的副本。已移出的其他截图保存在忽略的本地备份中，修改当前文件不清除 Git 历史版本。

两张图用于展示分屏与原生聊天布局，完整的模型调用、双向通信和 macOS 验收范围见发布检查清单。后续截图应使用独立演示项目和测试账号，检查私人路径、邮箱、凭据和日志后再用于文档。

通用图形来源与其他素材核对范围见 [素材记录](../asset-provenance.md)。

- 产品宣传片：用户于 2026-11-11 提供，61.312 秒、1920×1080、30 fps，H.264/AAC，30,888,074 字节；按用户确认原样发布，保留制作时的画面与片尾文字。原片通过 GitHub 原生视频附件托管，不纳入源码历史；中英文 README 顶部直接展示播放器，不要求进入 Release 下载。SHA-256：`0290e847ab64148a0cc6cbb83031944d8c06fa10188301e1eebb867328793216`。

  README 使用同一原片的 [GitHub 原生视频附件](https://github.com/user-attachments/assets/e0bcb281-1893-4359-b720-05e44055632c)，支持直接播放；未使用本地转码预览副本。

- `desktop-collaboration-team.png`：2026-11-11 用户提供的协作中心团队页截图，1122×793；原样收录，用于中英文 README 的团队成员、角色筛选、Runtime 管理与工作交接入口展示。截图中未见密钥或私人文件路径。SHA-256：`be6c1ace48eb458a19f890d0505aa15b009da3f9284ce6328ceb101b1ee2d9c9`。

- `collaboration-workflow.zh.svg` / `collaboration-workflow.en.svg`：将 README 原有 Mermaid 的四个节点及实现、审查、反馈、接手连线改为静态 SVG；用于去掉 GitHub 自动附带的图表缩放、移动、重置与复制工具，属于工作方式示意。
