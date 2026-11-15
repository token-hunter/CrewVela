Status: ready-for-human

# 完整移除皮肤功能

用户要求删除外观中的皮肤及整条实现。保留普通外观、透明度、桌宠、文件预览；移除专属前后端代码及授权，并验证旧持久化数据不会重新激活皮肤。

计划与证据：`.scratch/remove-skins/`。

## Comments

已完整移除皮肤入口、配置/导入、动态媒体和场景/音频渲染、Rust 专属命令与静态授权；普通外观、透明度、桌宠、文件预览保留。Node 全量 492 通过、1 跳过；Rust 全量 1130 通过、49 忽略。相关界面回归通过，旧皮肤配置不会再激活媒体。用户原始媒体文件未自动清理。

Windows EXE 构建、生产资源扫描、复制哈希、CLI 启动及隐私检查通过，交付 `output/CrewVela-no-skins-20261115/CrewVela.exe`。详见 [移除验证](../docs/remove-skins-verification-20261115.md)。
