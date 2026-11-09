# Custom Provider 模板设计文档

> CrewVela 内置一个空的「自定义」provider 模板
> 日期：2026-06-14

## 1. 目标

在内置 provider 列表（glm / kimi / minimax / deepseek 之外）加一个 `custom` 模板：所有字段留空，用户点编辑填自己的 API 端点（base_url / model 映射 / key），免去从零点「+」新建的步骤，提供一个现成的"自定义起点"。

## 2. 非目标

- 不做 provider 市场或在线模板下载。
- 不改变现有「+」新建自定义 provider 的流程（两条路并存）。
- custom 不内置任何具体厂商配置（保持空白模板）。

## 3. 数据模型（`providers.rs`）

`default_providers()` 末尾新增：

```rust
ProviderConfig {
    id: "custom",
    name: "自定义",
    base_url: "",            // 空，待用户填
    auth_token_ref: "custom",
    env: HashMap::new(),     // 空，用户填 model 映射
    timeout_ms: None,
    disable_nonessential_traffic: false,
    builtin: true,           // 不可删除，始终作为模板存在
}
```

`load()` 已有的迁移逻辑（补齐缺失的内置 provider）会自动让老 `providers.json` 出现 custom，无需额外迁移代码。

## 4. 启动校验（`lib.rs` `launch`）

点 custom 启动前，若 `provider.base_url` 为空，直接返回错误：

```
请先编辑 custom provider，填写 API 端点（base_url）
```

不 spawn claude，避免连到空端点产生不直观的报错。其他 provider（base_url 已填）不受影响。

## 5. 前端

- **ProviderList**：custom 与其他 builtin 同等显示、不可删除；当 `base_url` 为空时，徽章显示「未配置」（区别于「未设 Key」），提示用户先去编辑。
- **ProviderEditor**：custom 可正常编辑（base_url / env / key），无特殊逻辑。

## 6. 测试

- `default_providers` 含 `custom`，且其 `base_url` 为空、`env` 为空。
- `launch` 对 `base_url` 为空的 provider 返回错误字符串（不进入 spawn 路径）。

## 7. 实现（简述）

1. `providers.rs`：`default_providers` 加 custom 项；更新内置数量断言（4 → 5）。
2. `lib.rs`：`launch` 开头加 `base_url` 空校验。
3. `ProviderList.tsx`：徽章逻辑加「base_url 空 → 未配置」分支。
4. 单测随实现一起补。
