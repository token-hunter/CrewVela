Status: ready-for-human

# 协作中心500ms内显示

规格：.scratch/collaboration-open-perf-20261117/spec.md

## 实施与验证

拆除项目内容对Runtime/Skill慢扫描的等待；修复同ID初始快照作废等待3秒轮询；诊断与列表/快照离开窗口线程。
十次浏览器测量8至49ms（辅助IPC2500ms）；保留加载/失败提示、导航和成员操作回归。完整证据见规格目录verification.md。
Node486通过1跳过；Rust9项针对性测试通过；前端构建和差异检查通过。原生EXE端到端延迟尚待实机验收。
