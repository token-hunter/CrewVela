# 任务计划：Skill 中心 P0

## 目标
为 CrewVela 当前 8 个 Runtime 实现统一 Skill 资产库、安全分发矩阵、全局/项目/任务三级作用域、Skill 配置集及所有权保护。

## 当前阶段
已完成

## 阶段

### 阶段 1：模块边界与 Runtime 路径审计
- [x] 固定 SkillAsset / SkillDeployment / SkillSet 接口
- [x] 核对 8 个 Runtime 的发现根、部署根和扫描模式
- [x] 明确本地、Git、归档、纳管来源契约
- **状态：completed**

### 阶段 2：Rust 中央库和安全同步
- [x] SQLite 资产、部署、配置集模型
- [x] Skill 扫描、元数据、内容 Hash 和预览
- [x] 本地/Git/ZIP 安装与已存在 Skill 纳管
- [x] symlink → junction → copy 安全部署
- [x] 所有权、漂移、冲突、引用计数和安全撤下
- **状态：completed**

### 阶段 3：作用域和配置集
- [x] 全局、项目、任务/Agent 目标解析
- [x] Skill 配置集增删改和批量应用
- [x] 项目/角色绑定元数据和复现快照
- **状态：completed**

### 阶段 4：React Skill 中心
- [x] 中央资产库与来源/信任/预览
- [x] 8 Runtime 分发矩阵与真实状态
- [x] 作用域、项目、任务/Agent 选择器
- [x] 配置集管理和批量部署/撤下
- [x] 导入、扫描、纳管和删除确认
- **状态：completed**

### 阶段 5：验证
- [x] Rust 安全同步与数据库测试
- [x] 前端回归测试和生产构建
- [x] 浏览器布局检查
- [x] `git diff --check` 与差异审计
- **状态：completed**

## 范围外
- Skills 市场、远程更新检查/三方合并、Git 多设备备份、发布者签名、自动执行 Skill 脚本。

## 保护范围
- 保留尚未提交的 Git P0 工作台修改，不覆盖其他规划/临时文件。
- 撤下只处理 CrewVela 可证明所有权的目标；不覆盖用户现有普通目录、文件或外部链接。

## 错误记录
| 错误 | 尝试 | 处理 |
|---|---:|---|
| Cargo 首次下载新增依赖时系统凭据不可用 | 1 | 获得联网许可后成功锁定并下载依赖 |
| rusqlite 查询返回值生命周期晚于 Statement | 1 | 先收集到局部变量，再返回结果 |
