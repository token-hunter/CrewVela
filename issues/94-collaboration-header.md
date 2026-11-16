Status: ready-for-human

# 实现多智能体顶部第2版折叠成员工具栏

用户在注释fa88d259-26c7-4585-9c58-b84bbe570fb2中选择多智能体顶部三图中的第2版。已通过原会话消息msg_02884d0fa3e4e045016afa617c9e5487d19b88c0ccd8e7d3e4追溯实际展示顺序，视觉真值exec-33328071-f1a3-44a8-ad5f-f9bb609e9fbe.png。

## 验收

- 顶部紧凑项目选择/新建/设置，第二行保留目录与任务，右侧头像预览与“成员N”下拉；移除永久横向成员条，减少占高与滚动。
- 展开显示成员头像、名称、角色与勾选；与底部接收对象、@提及复用相同selected和toggleMember，原有Runtime禁用/不支持/busy限制保留。
- 项目切换、任务详情、项目设置、新建、讨论/执行模式、发送内容与后台命令不变；多个实例的弹层/焦点相互独立。
- 验证深浅色、窄侧栏、长标题/成员名、多成员、零成员、键盘、Esc/外点、滚动/尺寸变化及相关协作发送回归；实际图与选定图并列比较。
- 只在feature/major-redesign，保留上一任务隐私改动，不碰main/备份、不提交。

## Comments

- 2026-11-16：已确认此选择针对“多智能体顶部”原型，不是隐私或会话批量管理菜单。仅UI变更，默认继续交付含已完成隐私改动的Windows EXE。
- 2026-11-16：完成项目/目录/任务与折叠成员入口。顶部与底部共享selected/toggleMember；门户弹层支持外点、焦点离开、Esc、resize、长列表，Runtime/busy限制及原有回调保持。标题/弹层与选择图已全景和局部并排复核；首轮偏小的字号、头像和行距已修正。
- 验证：新页8个主题/视口组合、长名与27成员、空团队、busy、多实例、Runtime标识及键盘Enter/Tab/Space选人后精确发送通过；既有协作对话、明确成员选择、实例与隐私UI回归通过。Node完整476项：475通过、1跳过、0失败。浏览器使用显式IPC fixture，未调用真实Agent。
- 构建命令npm run tauri -- build --no-bundle；CARGO_BUILD_JOBS=1、临时CARGO_PROFILE_RELEASE_CODEGEN_UNITS=256；优化版5m03s成功，保留已有Rust警告与Vite大chunk警告。无Rust业务修改，不重复宣称全量Rust测试通过。
- 交付：output/CrewVela-collaboration-header-20261116-115602/CrewVela.exe；2026-11-16 11:55:37 +08:00构建；45262848字节；SHA256 A4499E12A7193996A6A2EA9937D316E71EA510FE2810EA68C83653FC9C1C2E72。隔离原生窗口响应正常，测试进程已关闭；原生界面各控件及真实Agent互聊仍未作端到端验收。代码未提交，main与备份未动，上一任务隐私源码哈希保持。
