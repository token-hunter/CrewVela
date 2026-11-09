# 发现：Tibo X 动态监控源

## 已验证的公开抓取能力
- 2026-08-19 对 `https://x.com/thsottiaux` 连续请求 3 次，均返回 HTTP 200。
- 每次服务器 HTML 均包含最近 5 个时间线条目，最新 ID 一致为 `2089941380336644295`。
- 可提取帖子 ID、`created_at_ms`、`full_text` 和原帖 URL。
- Jina Reader `https://r.jina.ai/https://x.com/thsottiaux` 能返回相同 5 条动态，可作为备用通道。
- `twitter-cli` 已安装，但本机 Cookie 失效并返回 401，不适合作为零配置运行依赖。

## 解析约束
- 不能宽泛匹配第一个 `rest_id`，否则会把用户 ID 当成帖子 ID。
- 应先从 `TimelineTimelineEntry:tweet-<id>` 或时间线 `entries:tweet-<id>` 获取有序帖子 ID，再定位对应 Tweet details。
- 只处理主页原创时间线条目，避免回复和其他用户内容污染。

## 分类建议
- `public_confirmed`：明确写出额度或限制已经 reset/refill/restored。
- `public_announcement`：明确写出 reset incoming、landing next hour、about to reset。
- `public_suspected`：来自 Tibo 且包含重置暗示，但未明确说明 Codex 额度，例如 “fancy new reset button”。

## 现有实现审计
- `CodexQuotaMonitor::check` 当前只并发抓取两个固定来源；在线/解析计数写入同一监控状态表。
- `load_source_diagnostics` 也使用固定二元数组，新增来源必须同时接入，否则 UI 不会显示。
- 每个来源以 `source` 为主键保存自己的 `last_event_id`，新来源首次检查会建立基线而不通知历史帖子。
- 通知表以 `public:<tweet_id>` 为全局主键，已经具备跨来源相同帖子自动去重能力。
- 当前解析器只适合普通站点 HTML；X 主页需专用时间线解析器，Jina Markdown 也需独立解析器。
- 前端状态文案写死 `/2`，新增来源后必须改为由诊断数组长度计算。

## 最终实现决策
- X 主页使用浏览器 User-Agent 获取服务器渲染时间线；失败后自动请求 Jina Reader。
- 新增 `latest_seen_id`，把“最新帖子”与“最近相关重置信号”分开保存，普通帖子不会覆盖重置游标。
- 第一次遇到相关帖子只建立来源基线；后续新相关帖子才通知。
- 相同帖子无论由 Tibo 主页还是两个监控站发现，均使用 `public:<tweet_id>` 去重。
- 三来源状态计数由前端诊断数组动态计算，不再写死 `/2`。
- 修复首次基线漏通知：Tibo 相关帖子在24小时内且通知表尚无该 ID 时，即使来源游标已建立也补发一次；旧帖子仍只作为基线。
