> 历史研究记录：皮肤功能已于 2026-11-15 按用户要求从 CrewVela 移除。本文保留当时源码研究，不代表当前产品能力。

# Codex-QQ-Skin「沉浸式女友 / 伙伴房间」源码实现与 CrewVela 复刻映射

> 研究对象：`REFERENCE_ROOT/Codex-QQ-Skin`，本地版本 `2.6.3`（`package.json`）。研究日期：2026-08-12。本文只记录源码事实和适配方案，不包含产品代码修改。

## 结论

Codex-QQ-Skin 中并不存在 Live2D、Spine、Lottie、视频数字人、表情捕捉、唇形同步或真人语音对话实现。仓库标题所说的“内置沉浸式女友”，源码上实际是**把一张人物写真作为皮肤素材，分层铺到整个 Codex 窗口，并对工作区面板做透明化和安全区排版**；另有一套独立的“Codex 伙伴房间”，使用**一张静态 PNG + CSS 位移/旋转/缩放动画**表现呼吸、工作、庆祝、困倦等状态。

因此：

- 若“直接复刻这个项目的效果”，CrewVela 应首先把当前小型悬浮聊天卡改成**全窗口沉浸式人物场景层**，而不是继续增强小卡片。
- 可以复刻人物随界面皮肤出现、面板半透明、人物位于右侧/底部、内容避让人物、安全区与焦点适配等核心观感。
- 不能把参考项目描述成“会说话的动态真人”。精确复刻后仍是静态写真或轻量 CSS 动效；CrewVela 现有的语音阶段、音量驱动嘴型和回复情绪能力反而比参考项目更多，应作为增强保留。
- 参考项目的 CDP/Electron 注入层不能原样复制到 Tauri/React 的 CrewVela；要复制的是状态、资源分层、布局和安全校验机制，并用 React 组件与 Tauri 命令重写宿主集成。

## 1. 源码中其实有两套“伙伴”视觉

### 1.1 “沉浸式女友”：人物写真就是皮肤场景

README 把项目称为“Codex 皮肤生成器（内置沉浸式女友）”，同时明确自定义模式是上传任意图片或生成完整分层皮肤（`REFERENCE_ROOT/Codex-QQ-Skin\README.md:3-12`）。源码没有单独的 `Girlfriend`、Live2D 或视频组件；实际入口是皮肤加载和 CSS 场景合成。

有两种资源格式：

1. **schema v1 单图皮肤**
   - `theme.json` 至少包含 `schemaVersion: 1` 和单张 `image`。
   - `art.focusX/focusY` 控制焦点；`safeArea` 控制文字放左、右、中或全遮罩；`taskMode` 控制任务页使用 ambient/banner/off。
   - 普通竖图、方图、横图使用 `contain`，只有真正的超宽图才使用 `cover`，避免人物头部或身体被裁掉（`assets/renderer-inject.js:560-598`）。
   - 首页把原生新建任务区变成 248～376px 的场景画框，人物图作背景，并根据安全区给文字加可读性遮罩（`assets/custom-skin.css:595-695`）。超宽图会直接覆盖整个窗口，侧栏和主工作区变成渐变透明层（`assets/custom-skin.css:820-908`）。

2. **schema v2 深度分层皮肤**
   - 固定资源槽：`background`、`foregroundRight`、`sidebarCharacter`、`watermark`、`brandEmblem`、`avatar`，只有 `background` 必填（`scripts/deep-theme-core.mjs:6-17, 91-112`）。
   - `foregroundRight` 支持宽度、右偏移、底偏移、透明度；侧栏人物支持尺寸、Y 位置和透明度；水印支持宽度、X/Y 位置和透明度（`scripts/deep-theme-core.mjs:125-141`）。
   - 注入时把资源映射为 `--dream-deep-right/sidebar/watermark/brand/avatar` 等 CSS 变量，并把布局数字转为对应 CSS 变量（`assets/renderer-inject.js:2991-3047`）。
   - 渲染层次是：`body::before` 全屏背景、`body::after` 右下前景人物、主区水印、侧栏低透明人物；侧栏、输入框、消息卡和顶栏使用半透明面板和模糊，保证人物可见但文字可读（`assets/custom-skin.css:1242-1350`）。

这套机制就是用户在站点截图里感知到的“女友进入工作空间”：人物不是一张浮在输入框上方的卡片，而是皮肤构图的一部分。

### 1.2 “Codex 伙伴房间”：静态机器人 PNG + CSS 状态动画

“伙伴房间”是 2.6.0 新增的另一功能，README 明确它位于右下角，会随任务状态、时间和互动变化，并附带 GitHub 项目盲盒（`README.md:45-51`）。

- 入口函数为 `ensureCompanion()`，直接创建 `<section id="codex-qq-skin-companion">` 并挂到 `document.body`。房间内的窗户、月亮、云、书架、台灯、显示器、书桌、咖啡杯、盆栽、彩纸和面板大多是 HTML/CSS 元素（`assets/renderer-inject.js:2323-2379`）。
- 角色只是一张 `assets/codex-pet.png`，尺寸 530×742、约 343KB；在房间里显示为 87×118px，`object-fit: contain`（`assets/qq-skin.css:1166-1183`）。
- 没有角色帧序列。默认是 4.8 秒呼吸缩放；运行中对整个角色容器做 0.42 秒往复动作；完成时撒彩纸；离线时灰化倾斜；阅读、开心、兴奋、困倦和休息也只是容器旋转/位移/滤镜（`assets/qq-skin.css:1360-1408, 1658-1670, 1895-1911`）。
- `data-status=running|approval|completed|offline|idle` 驱动状态动画；`data-room-phase=morning|day|evening|night` 驱动窗外时间；`data-room-mood=happy|curious|sleepy|excited|reading|break` 驱动临时动作；`data-room-motion=off` 会关闭房间所有动画。
- 它固定贴右侧摘要面板，尺寸随右侧面板宽度变化，默认不可见，只有 `.is-visible` 才显示；进入设置页会隐藏，窄屏也会隐藏。它**不可拖动**（`assets/qq-skin.css:838-900`）。

## 2. 用户要求的各项能力在参考源码中的真实情况

| 能力 | 参考源码实际实现 | 证据/限制 |
|---|---|---|
| 入口组件 | 无 React 组件；`ensureCompanion()` 动态建 DOM；沉浸女友走皮肤加载路径 | `renderer-inject.js:2323-2439`、`injector.mjs:309-358` |
| 窗口/覆盖层 | 皮肤用 `body::before/after` 分层；伙伴房间用 fixed 右下卡片 | `custom-skin.css:1242-1350`、`qq-skin.css:838-874` |
| 角色资源 | 沉浸皮肤支持单图或 6 个分层 PNG/JPEG/WebP；房间角色是一张 PNG | `deep-theme-core.mjs:6-20`、`codex-pet.png` |
| Live2D/视频 | **没有** | 全仓库无 Live2D/Spine/Lottie/视频角色依赖或运行时 |
| 动画 | CSS 对整张静态角色图做呼吸、摆动、旋转、灰化；场景雨层另用 Canvas | `qq-skin.css:1166-1183, 1360-1408, 1895-1911` |
| 表情状态 | 没有换脸/表情帧；只有 mood/status 数据属性对应姿态与文案 | `renderer-inject.js:2069-2082, 2297-2319` |
| 鼠标交互 | 点击角色随机 mood；窗户切时间；咖啡杯 5 分钟休息；书架/灯/显示器/盆栽抽盲盒 | `renderer-inject.js:2261-2321` |
| 键盘交互 | 房间动作支持 Enter/Space | `renderer-inject.js:2413-2428` |
| 拖动位置 | **没有** | 伙伴固定于右摘要面板；沉浸人物固定于场景布局 |
| 声音 | 任务完成、授权、上线提示音；WebAudio 合成 + 一段本地 MP3；首次用户交互后解锁 | `renderer-inject.js:801-832, 1080-1125` |
| 对话/语音联动 | **没有 TTS、口型或回复内容联动** | 声音监控只识别运行/授权/完成/在线状态 |
| 设置持久化 | sound、room-motion 和盲盒偏好放 localStorage；皮肤配置保存在 `theme.json`/主题目录 | `renderer-inject.js:805-815, 2297-2300`；`deep-theme.mjs` |
| IPC | 注入脚本通过 `window.electronBridge.sendMessageFromView({type:'avatar-overlay-open'})` 打开 Codex 原生宠物；终端通过点击原生 DOM 按钮 | `renderer-inject.js:2044-2067` |
| 宿主技术 | Electron 官方 Codex + 本机 CDP 注入，不是 Tauri | `injector.mjs:82-130` |

## 3. 注入架构与为什么不能原样搬到 CrewVela

参考项目不拥有 Codex 源码，所以采用旁路注入：

1. 只接受本机 loopback、指定端口、`ws:`、`/devtools/page/<安全 ID>` 的 CDP 地址，目标还必须是 `type: page` 且 URL 以 `app://` 开头（`scripts/injector.mjs:82-105`）。
2. 通过 WebSocket 建立 CDP 会话，并把样式和渲染脚本注册到 `Page.addScriptToEvaluateOnNewDocument`。
3. 注入层用 `MutationObserver`、`ResizeObserver`、路由点击监听和 requestAnimationFrame 调度，在官方 Codex SPA 重绘后不断找回目标 DOM（`assets/renderer-inject.js:3661-3704`）。
4. 注入资源先变为 data URL，浏览器内再转 Blob URL；卸载和重新注入时逐一 `URL.revokeObjectURL`（`renderer-inject.js:151-171, 3640-3657`）。

CrewVela 自己就是 React/Tauri 应用，已经拥有组件树，不需要也不应复制 CDP、选择器猜测、MutationObserver 修补和 Electron bridge。相同功能应在 React 渲染树中直接实现，Tauri 只负责文件导入和持久化。

## 4. CrewVela 当前实现与差距

### 4.1 当前已有能力

- `src/components/DesktopCompanion.tsx:25-29` 把 floating/sidebar/immersive 都定义成 280×88 或 340×108 的卡片；所谓 immersive 只是卡片更大，并非全窗口场景。
- `DesktopCompanion.tsx:31-121, 144-180` 已支持窗口边界/右侧对话框边界避让、拖动、resize 重新夹紧和位置持久化。这些能力参考项目没有。
- `DesktopCompanion.tsx:52-84` 使用 4×2 共 8 帧的 motion sheet：闭嘴、眨眼、小/中/大嘴型、开心、担忧、专注。
- `src/companionMotion.ts:1-75` 已有 `idle/listening/processing/speaking/error` 阶段，音量阈值口型，以及根据回复文本判断 warm/happy/concerned/focused。参考项目没有声音驱动口型。
- `src/companionSettings.ts:32-52, 72-92` 已持久化角色、昵称、性格、声音、显示模式、主动程度、任务反馈、休息提醒、安静时段、皮肤联动、动效/声音、导入写真和位置；`App.tsx:403-425` 统一保存。
- `src/components/CompanionSettingsPanel.tsx:41-58` 已支持图片导入，当前只按 MIME 和 2.5MB 限制后把 data URL 放进 localStorage。

### 4.2 用户感知“不像参考项目”的根因

当前 `DesktopCompanion` 永远渲染为带头像、昵称和提示文字的小卡片，且浮在最高层；参考项目“沉浸式女友”的人物属于背景/前景场景，主界面面板会为人物留空并半透明。两者不是同一种布局。继续给小卡片增加眨眼或嘴型，无法得到参考图的沉浸效果。

## 5. 可直接复刻的文件级映射

| Codex-QQ-Skin 源文件/机制 | CrewVela 目标文件 | 复刻方式 |
|---|---|---|
| `assets/custom-skin.css:1242-1350` 分层场景 | **新增** `src/components/CompanionSceneLayer.tsx`、`src/companionScene.css` | React 输出 background/foreground/sidebar/watermark 层；CSS 管理层级、透明面板、安全区和响应式 |
| `renderer-inject.js:560-598` 图片宽高/焦点/安全区 | **新增** `src/companionScene.ts` | 用 `Image.decode()` 取宽高，推导 portrait/square/landscape/wide、contain/cover、focus 和 safeArea |
| `deep-theme-core.mjs:6-172` schema v2 | **新增** `src/companionScenePack.ts` | 定义 TS schema、白名单字段、数值范围、颜色、资产槽和标准化函数 |
| `deep-theme-core.mjs:175-247` 安全读取 | **新增/扩展** `src-tauri/src/commands/companion_scene.rs` | Tauri 后端完成 realpath、目录包含关系、拒绝软链接、稳定 stat、大小/尺寸/格式验证 |
| `deep-theme.mjs:162-236` `.codexskin` 导入 | **新增** CrewVela 场景包导入命令 | 限 entry 数、解包总量、只允许平铺文件名、拒绝重复/软链接、校验 CRC、临时目录验证后原子安装 |
| `renderer-inject.js:2991-3047` 主题到 CSS 变量 | `App.tsx` + `CompanionSceneLayer.tsx` | 不写 `documentElement.style`；将标准化配置转换成组件内 CSS custom properties |
| `renderer-inject.js:2069-2082` status/phase | `DesktopCompanion.tsx` 或抽取 `useCompanionState.ts` | 直接从 sessions、语音事件生成 scene 的 data 属性/类名 |
| `renderer-inject.js:801-1125` 提示音 | 现有通知/声音服务 | 只借鉴首次交互解锁、开关和音量；不要重复造第二套音频系统 |
| `renderer-inject.js:2323-2439` 伙伴房间 | **可选新增** `CompanionRoom.tsx` | 只有用户明确还要“房间/盲盒”时再搬；不属于沉浸女友核心 |
| `qq-skin.css:838-1911` 房间样式 | **可选新增** `companionRoom.css` | React 化 data-status/phase/mood/motion；不要沿用全局 ID 和乱码文案 |
| `window.electronBridge` | React 回调 / Tauri command | 打开 CrewVela 自己的宠物面板或终端，不使用 Electron bridge |
| CDP + MutationObserver | 不迁移 | React 状态更新天然重绘；保留 resize/容器测量即可 |

## 6. 必须适配的产品设计

### 6.1 重新定义“沉浸”显示模式

建议将 `displayMode: immersive` 从“大一点的悬浮卡片”改为真正的场景模式：

- `floating`：保留现在可拖动小卡片。
- `sidebar`：保留受侧栏约束的紧凑形态。
- `immersive`：不渲染卡片边框、昵称气泡和占位文案；在应用主背景层渲染人物写真，人物默认锚定右下，所有点击默认穿透，避免挡住快速指挥和项目协作输入框。
- 右侧对话框展开时，按其实际左边界调整人物的 `right` 或整体降为低透明度，不应覆盖对话内容。
- 小窗口按断点降低人物尺寸/透明度，必要时隐藏前景人物，只保留场景背景。

### 6.2 把“角色”和“场景包”分开

当前 `customPortrait` 只是一个 data URL。为复刻参考项目，应增加：

- 角色图：透明背景的 `foregroundRight`，用于沉浸人物前景。
- 场景背景：可选全屏背景。
- 可选侧栏淡化人物、水印、品牌图。
- `safeArea/focusX/focusY/fit`，确保人物和输入区域不会冲突。
- 单图导入仍可自动生成一个临时 schema v1 场景；完整导入使用受控场景包。

### 6.3 状态/声音复用 CrewVela，不能照搬参考项目的弱实现

- `companionMotion.ts` 已经把语音音量映射成嘴型，应该继续作为唯一运动状态源。
- 在沉浸场景中，现有 motion sheet 可作为人物脸部叠层；或者只做非常轻微的呼吸、视线和亮度变化。
- 参考项目没有 TTS 和嘴型，不能为了“照搬”而删除 CrewVela 已有能力。
- 系统提示音走已有通知设置，角色语音走现有语音/TTS 链路，避免同时创建两个 AudioContext、两个音量开关。

### 6.4 可访问性和交互

- 沉浸人物层默认 `pointer-events: none`；若提供互动热点，必须显式开启且不能覆盖输入框。
- 支持 `prefers-reduced-motion` 和现有 `skinMotionEnabled`。
- 角色可拖动只应存在于 floating 模式；immersive 模式应使用可配置锚点/偏移，避免用户误拖破坏整体构图。
- 所有自定义设置应能“一键恢复默认/关闭场景”，同时恢复应用之前的外观。

## 7. 资源导入与安全要求

参考项目的深度主题导入比 CrewVela 当前 FileReader 严格得多，值得完整复刻：

- 只允许 PNG/JPEG/WebP；单文件最大 16MB、包总计 64MB、配置最大 1MB（`deep-theme-core.mjs:6-20`）。
- 文件名必须是目录内的平铺名称，拒绝斜杠、控制字符和路径穿越；未知字段直接拒绝（`deep-theme-core.mjs:22-77`）。
- 必须有背景图；每个资产使用不同文件名（`deep-theme-core.mjs:91-112`）。
- realpath 后再次确认仍在主题目录；拒绝软链接、目录和未声明的额外文件（`deep-theme-core.mjs:175-247`）。
- 打开前后比较设备号、inode、大小和时间，避免读取过程中被替换；读取后验证真实图片格式、尺寸和像素上限。
- `.codexskin` 限最多 32 个 ZIP entry，只接受 stored 条目，限制膨胀大小，拒绝重复/危险名称/软链接，核验 local/central 文件名与 CRC，在临时目录完整验证后再安装（`deep-theme.mjs:162-236`）。
- 写入使用临时文件 + 独占创建 + 原子 rename；导入失败删除临时目录（`deep-theme.mjs:45-52, 224-236`）。
- CrewVela 不应把完整高清写真长期写进 localStorage；应复制到 Tauri app-data 下的受控目录，前端只保存场景 ID 和相对资产 ID。

## 8. 建议的复刻顺序与验收口径

### 第一阶段：精确复刻“沉浸式女友”观感

1. 增加 `CompanionSceneLayer`，实现单图和 background + foregroundRight 两层。
2. 把 `immersive` 改为无卡片的全窗口场景，主面板、右侧对话框、输入框使用可读的半透明表面。
3. 实现 focus/safeArea/contain/cover 和右侧对话框避让。
4. 保留现有 motion sheet、语音阶段和音量驱动；无动图资源时只做轻量呼吸。
5. 支持关闭、恢复外观、减少动效。

验收重点：人物不再挡输入框；窗口缩放、展开/收起右侧对话框均不会盖住文字；沉浸模式没有多余卡片边框和气泡；关闭后完全恢复原界面。

### 第二阶段：安全的场景包导入

按 schema v2 实现受控目录和 `.codexskin` 导入，优先复刻安全边界，随后再提供可视化焦点和偏移调节。

### 第三阶段（可选）：伙伴房间

只有用户还希望拥有右下房间、时间变化、任务状态、休息计时或 GitHub 盲盒时，才移植 `CompanionRoom`。这部分不能替代沉浸女友场景，也不应与现有水果宠物入口混为一体。

## 9. 不应声称已从参考项目复刻的能力

- 真人说话、口型同步、情绪表情切换。
- Live2D、3D 数字人或视频驱动。
- 对话模型与角色人格。
- 角色自由行走或拖动。
- 摄像头/麦克风表情捕捉。

这些能力都不在 Codex-QQ-Skin 2.6.3 源码中。若 CrewVela 继续实现它们，应标明是 CrewVela 自身增强，而不是“直接复刻参考源码”。

