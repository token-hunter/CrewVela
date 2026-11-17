# 高清欢迎角色素材

内置 imagegen 根据已选定的 Clawd 程序员、ZCode 电玩小怪、鲸鱼娘造型重制。每张母版为 1774×887，包含两个原生 887×887 姿态；未将原始约 222px 图集放大充数。

每个角色目录：

- `idle.webp`：共享待机姿态。
- `action-0.webp` 至 `action-7.webp`：保留的原始高清端点，按 2×1 排列，不再直接作为运行时动作图集。
- `flow.png`、`visibility.png`、`manifest.json`：保留的上一版中点数据与原始素材 SHA256，供回溯和对照。
- `generation.json`：内置工具模式、最终提示词与源图审计记录。
- `motion-0.webp` 至 `motion-7.webp`：运行时动作图集。前两格仍是原始端点像素，随后按需包含已经检查的原生高清中间姿态；每格 887px，具体行列见新清单。
- `motion-flow.png`、`motion-visibility.png`、`motion-manifest.json`：连续姿态路径、每个短段在 0/0.25/0.5/0.75/1 的位移/遮挡、局部道具或手部的对应数据。图集最大边不超过 4096px，运行时也检查实际 GPU 限制。

新增母版保存在本目录的 `transition-sources/`，其生成来源记录见 `transition-sources/generation.json`。失败试验、低分辨率候选和未采用身体补图不属于产品素材。个别生成母版宽 1773px；仅在明确全透明的右边界补 1 列透明画布，并在清单记录原尺寸及 `transparentPaddingRight`，不缩放角色像素。

WebP 是保留像素和透明通道的无损导出。运行时只解码/驻留当前及下一动作，取消过时的加载请求，非活动时释放 GPU 纹理；不加载整套高清动作图。新增姿态增加了单动作纹理字节数，缓存上限仍是两个动作，不能将“纹理数量相同”解释为“内存用量不变”。运行期只引用 `motion-*` 动作图，不将原始端点及母版重复打入前端包。动作资源和模型均无需联网调用，模型不进入产品包。

开发工具 `scripts/build-mascot-hd.py` 使用项目 `.scratch/mascot-hd-20261117/source-batch-*.json` 指向的原始生成文件，复制母版到同目录的 `masters/` 后导出素材；开发用 RIFE 模型沿用 `scripts/build-mascot-flow.py` 的配置。源 PNG 不存在时，可从本目录无损 WebP 恢复姿态像素；重制新姿态时使用对应的生成记录。

当前运行时数据可直接从保留的 WebP 端点、项目内 PNG 母版及持久配置重建：

```powershell
rtk proxy python scripts/build-mascot-transitions.py --spec scripts/mascot-transition-sources.json
```

开发依赖为 Pillow、NumPy、OpenCV、PyTorch，以及通过 SHA256 校验的 RIFE 4.25 模型（`MASCOT_RIFE_MODEL` 指向其 `train_log`）。`.scratch/mascot-overlap-fix/field-cache/` 只是可删除的数值缓存；清掉它不会失去母版或路径配置。构建先在隔离目录完成校验，再发布完整数据。

`scripts/build-mascot-rigid.py` 对 CLEAR 牌的共同投影平面和敲键盘末段的手部做局部配准，分离相互遮挡的采样域。播放器保留完整身体绘制，再按预乘 alpha 绘制局部前景，并恢复混合状态；不以整图淡化、硬切或降低帧率代替连续运动。验收包含每个新增短段内部的分位帧，不能只检查新绘制的关键姿态。

短段时长按其实际预乘像素变化量分配：`routeStops` 描述逻辑路径内各短段的累计时长，`segmentStops` 对应模型采样时刻的累计变化量。它们只调整同一路径上的速度，不改变外层动作时长、姿态或反向恢复路径。测试仍在每个几何短段内部采样；不能用非等分时长避开检查。

动作图通过 `fetch → Blob → createImageBitmap` 异步解码和预乘转换后上传，过时请求会中止，所有 Bitmap 在上传或取消后关闭。手/道具 matte 与遮挡权重共享灰度图集中的不同 tile，避免为了局部 matte 将整张权重纹理扩成 RGBA。

性能记录区分初次加载与稳定播放：Windows Chrome/NVIDIA 5060 Ti 的 27 秒独立播放回归中，rAF p95 为 18.1ms，稳定播放期间无超过 50ms 的帧间隔或 longtask。大动作图集的 GPU 上传由约 105–110ms 降至约 10–18ms；初次静态图片/数值图加载仍可能产生启动任务，不能将播放数据表述为整个启动过程无阻塞。尚未以原生 EXE 或 macOS WebView 的实际运行替代这些浏览器证据。

高清素材的像素数不等于每一帧都经过手工绘制。大幅遮挡时的连续插值仍可能出现局部软化，验收需同时查看静态细节和完整动作。
