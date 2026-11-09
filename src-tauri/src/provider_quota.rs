//! Official account quota adapters, based on CC Switch's MIT-licensed coding_plan/balance adapters.
//! See THIRD_PARTY_NOTICES/provider-quota.md. No configured URL or custom script is executed.
use crate::{providers::ProviderConfig, secrets};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    label: String,
    used_percent: Option<f64>,
    remaining_percent: Option<f64>,
    reset_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Balance {
    amount: f64,
    currency: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuota {
    status: String,
    message: Option<String>,
    updated_at: Option<i64>,
    windows: Vec<QuotaWindow>,
    balances: Vec<Balance>,
}
impl ProviderQuota {
    fn status(status: &str, message: &str) -> Self {
        Self {
            status: status.into(),
            message: Some(message.into()),
            updated_at: None,
            windows: vec![],
            balances: vec![],
        }
    }
    fn success(windows: Vec<QuotaWindow>, balances: Vec<Balance>) -> Self {
        Self {
            status: "ok".into(),
            message: None,
            updated_at: Some(chrono::Utc::now().timestamp_millis()),
            windows,
            balances,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Adapter {
    Kimi,
    Glm(bool),
    MiniMax(bool),
    DeepSeek,
}
fn adapter(provider: &ProviderConfig) -> Option<Adapter> {
    let url = reqwest::Url::parse(&provider.base_url).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    match url.host_str()? {
        "api.kimi.com" if url.path() == "/coding" || url.path().starts_with("/coding/") => {
            Some(Adapter::Kimi)
        }
        "open.bigmodel.cn" => Some(Adapter::Glm(false)),
        "api.z.ai" => Some(Adapter::Glm(true)),
        "api.minimaxi.com" => Some(Adapter::MiniMax(false)),
        "api.minimax.io" => Some(Adapter::MiniMax(true)),
        "api.deepseek.com" => Some(Adapter::DeepSeek),
        _ => None,
    }
}
fn endpoint(adapter: Adapter) -> &'static str {
    match adapter {
        Adapter::Kimi => "https://api.kimi.com/coding/v1/usages",
        Adapter::Glm(false) => "https://open.bigmodel.cn/api/monitor/usage/quota/limit",
        Adapter::Glm(true) => "https://api.z.ai/api/monitor/usage/quota/limit",
        Adapter::MiniMax(false) => {
            "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains"
        }
        Adapter::MiniMax(true) => "https://api.minimax.io/v1/api/openplatform/coding_plan/remains",
        Adapter::DeepSeek => "https://api.deepseek.com/user/balance",
    }
}
fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n: &f64| n.is_finite())
}
fn percent(value: f64) -> Option<f64> {
    if (0.0..=100.0).contains(&value) {
        Some(value)
    } else {
        None
    }
}
fn reset(value: &Value) -> Option<i64> {
    value.as_i64().filter(|n| *n > 0).or_else(|| {
        chrono::DateTime::parse_from_rfc3339(value.as_str()?)
            .ok()
            .map(|time| time.timestamp_millis())
    })
}
fn window(label: &str, used: f64, reset_at: Option<i64>) -> Option<QuotaWindow> {
    Some(QuotaWindow {
        label: label.into(),
        used_percent: Some(percent(used)?),
        remaining_percent: Some(100.0 - used),
        reset_at,
    })
}
fn kimi_window(label: &str, value: &Value) -> Option<QuotaWindow> {
    let limit = number(&value["limit"])?;
    let remaining = number(&value["remaining"])?;
    if limit <= 0.0 {
        return None;
    }
    let reset_at = match value["resetTime"].as_i64() {
        Some(timestamp) if timestamp > 0 && timestamp < 1_000_000_000_000 => {
            timestamp.checked_mul(1000)
        }
        _ => reset(&value["resetTime"]),
    };
    window(label, (limit - remaining) / limit * 100.0, reset_at)
}
fn glm_windows(value: &Value) -> Vec<QuotaWindow> {
    let mut slots: [Option<QuotaWindow>; 2] = [None, None];
    let mut fallback = vec![];
    if let Some(limits) = value["data"]["limits"].as_array() {
        for limit in limits {
            let Some(kind) = limit["type"].as_str() else {
                continue;
            };
            if !kind.eq_ignore_ascii_case("TOKENS_LIMIT")
                && !kind.eq_ignore_ascii_case("CREDIT_LIMIT")
            {
                continue;
            }
            let Some(mut item) = number(&limit["percentage"])
                .and_then(|p| window("套餐额度", p, reset(&limit["nextResetTime"])))
            else {
                continue;
            };
            let slot = match limit["unit"].as_i64() {
                Some(3) => Some(0),
                Some(6) => Some(1),
                _ => None,
            };
            if let Some(index) = slot {
                item.label = if index == 0 { "5 小时" } else { "7 天" }.into();
                if slots[index].is_none() {
                    slots[index] = Some(item);
                }
            } else {
                fallback.push(item);
            }
        }
    }
    // Legacy plans omit unit. Fill missing windows only, never reclassify explicit units.
    fallback.sort_by_key(|item| (item.reset_at.is_some(), item.reset_at.unwrap_or(i64::MIN)));
    for mut item in fallback {
        if let Some(index) = slots.iter().position(Option::is_none) {
            item.label = if index == 0 {
                "5 小时（推定）"
            } else {
                "7 天（推定）"
            }
            .into();
            slots[index] = Some(item);
        }
    }
    slots.into_iter().flatten().collect()
}
fn parse(adapter: Adapter, value: &Value) -> Result<ProviderQuota, &'static str> {
    let mut windows = vec![];
    let mut balances = vec![];
    match adapter {
        Adapter::Kimi => {
            if let Some(limits) = value["limits"].as_array() {
                for limit in limits {
                    if let Some(w) = kimi_window("5 小时", &limit["detail"]) {
                        windows.push(w);
                    }
                }
            }
            if let Some(w) = kimi_window("7 天", &value["usage"]) {
                windows.push(w);
            }
        }
        Adapter::Glm(_) => {
            windows = glm_windows(value);
        }
        Adapter::MiniMax(_) => {
            if let Some(general) = value["model_remains"]
                .as_array()
                .and_then(|list| list.iter().find(|item| item["model_name"] == "general"))
            {
                if let Some(w) = number(&general["current_interval_remaining_percent"])
                    .and_then(|p| window("5 小时", 100.0 - p, reset(&general["end_time"])))
                {
                    windows.push(w);
                }
                if general["current_weekly_status"] == 1 {
                    if let Some(w) = number(&general["current_weekly_remaining_percent"])
                        .and_then(|p| window("7 天", 100.0 - p, reset(&general["weekly_end_time"])))
                    {
                        windows.push(w);
                    }
                }
            }
        }
        Adapter::DeepSeek => {
            if let Some(list) = value["balance_infos"].as_array() {
                for balance in list {
                    if let (Some(amount), Some(currency)) = (
                        number(&balance["total_balance"]),
                        balance["currency"].as_str(),
                    ) {
                        if matches!(currency, "CNY" | "USD") {
                            balances.push(Balance {
                                amount,
                                currency: currency.into(),
                            });
                        }
                    }
                }
            }
        }
    }
    if windows.is_empty() && balances.is_empty() {
        return Err("供应商响应缺少有效额度数据");
    }
    Ok(ProviderQuota::success(windows, balances))
}

type Cache = HashMap<String, (String, ProviderQuota)>;
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
pub async fn get(provider: &ProviderConfig, force: bool) -> ProviderQuota {
    let Some(adapter) = adapter(provider) else {
        return ProviderQuota::status("unsupported", "此供应商暂不支持账户额度查询");
    };
    let key = match secrets::get(&provider.auth_token_ref) {
        Ok(Some(key)) if !key.trim().is_empty() => key,
        Ok(_) => return ProviderQuota::status("unconfigured", "请先配置模型 API Key"),
        Err(_) => return ProviderQuota::status("error", "无法读取系统凭据管理器中的 API Key"),
    };
    // Keys remain in keychain; cache identifiers contain only a digest, never the credential.
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}\0{}\0{}",
                provider.base_url, provider.auth_token_ref, key
            )
            .as_bytes()
        )
    );
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let previous = cache.lock().ok().and_then(|cache| {
        cache
            .get(&provider.id)
            .filter(|(hash, _)| hash == &fingerprint)
            .map(|(_, quota)| quota.clone())
    });
    if !force {
        if let Some(previous) = &previous {
            if chrono::Utc::now().timestamp_millis() - previous.updated_at.unwrap_or(0) < 300_000 {
                return previous.clone();
            }
        }
    }
    let result = query(adapter, &key).await;
    // Do not publish an old account response after an edit replaced its key or endpoint.
    let current = crate::providers::get(&provider.id).ok().flatten();
    if !current.as_ref().is_some_and(|current| {
        current.base_url == provider.base_url && current.auth_token_ref == provider.auth_token_ref
    }) || secrets::get(&provider.auth_token_ref)
        .ok()
        .flatten()
        .as_deref()
        != Some(key.as_str())
    {
        return ProviderQuota::status("error", "模型凭据已变更，请重新刷新额度");
    }
    match result {
        Ok(quota) => {
            if let Ok(mut cache) = cache.lock() {
                cache.insert(provider.id.clone(), (fingerprint, quota.clone()));
            }
            quota
        }
        Err(message) => {
            let mut quota = previous.unwrap_or_else(|| ProviderQuota::status("error", message));
            quota.status = "error".into();
            quota.message = Some(message.into());
            quota
        }
    }
}
async fn query(adapter: Adapter, key: &str) -> Result<ProviderQuota, &'static str> {
    let client = crate::general_settings::configure_http_client(reqwest::Client::builder())
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建额度查询客户端")?;
    let auth = if matches!(adapter, Adapter::Glm(_)) {
        key.to_string()
    } else {
        format!("Bearer {key}")
    };
    let mut response = client
        .get(endpoint(adapter))
        .header(reqwest::header::AUTHORIZATION, auth)
        .header("Accept-Language", "zh-CN,zh;q=0.9")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "额度查询超时"
            } else {
                "额度服务连接失败"
            }
        })?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 | 403 => "额度查询认证失败，请检查 API Key 或套餐权限",
            429 => "额度查询频率受限，请稍后重试",
            _ => "额度服务返回异常状态",
        });
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取额度响应失败")? {
        if bytes.len() + chunk.len() > 256 * 1024 {
            return Err("额度响应过大");
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "额度响应格式无效")?;
    parse(adapter, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn kimi_reports_used_not_remaining() {
        let quota = parse(Adapter::Kimi, &json!({"limits":[{"detail":{"limit":100,"remaining":75,"resetTime":"2026-10-02T12:00:00Z"}}],"usage":{"limit":1000,"remaining":420}})).unwrap();
        assert_eq!(quota.windows[0].used_percent, Some(25.0));
        assert!((quota.windows[1].used_percent.unwrap() - 58.0).abs() < 1e-9);
        assert!(quota.windows[0].reset_at.is_some());
    }
    #[test]
    fn glm_percentage_is_utilization() {
        let quota = parse(Adapter::Glm(false), &json!({"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"percentage":12,"nextResetTime":1790942400000i64},{"type":"CREDIT_LIMIT","unit":6,"percentage":58}]}})).unwrap();
        assert_eq!(quota.windows[0].used_percent, Some(12.0));
        assert_eq!(quota.windows[1].label, "7 天");
    }
    #[test]
    fn kimi_reset_accepts_seconds_milliseconds_and_iso() {
        for value in [
            json!(1790942400),
            json!(1790942400000i64),
            json!("2026-10-02T12:00:00Z"),
        ] {
            let item = kimi_window(
                "5 小时",
                &json!({"limit":100,"remaining":100,"resetTime":value}),
            )
            .unwrap();
            assert_eq!(item.reset_at, Some(1790942400000));
        }
    }
    #[test]
    fn minimax_inverts_remaining_and_honors_weekly_status() {
        let mut value = json!({"model_remains":[{"model_name":"general","current_interval_remaining_percent":98,"end_time":1790942400000i64,"current_weekly_status":0,"current_weekly_remaining_percent":91}]});
        assert_eq!(
            parse(Adapter::MiniMax(false), &value)
                .unwrap()
                .windows
                .len(),
            1
        );
        value["model_remains"][0]["current_weekly_status"] = json!(1);
        let quota = parse(Adapter::MiniMax(false), &value).unwrap();
        assert_eq!(quota.windows[0].used_percent, Some(2.0));
        assert_eq!(quota.windows[1].used_percent, Some(9.0));
    }
    #[test]
    fn glm_legacy_fallback_does_not_reverse_explicit_windows() {
        let quota = parse(Adapter::Glm(false), &json!({"data":{"limits":[{"type":"TOKENS_LIMIT","unit":6,"percentage":58,"nextResetTime":1},{"type":"tokens_limit","percentage":0}]}})).unwrap();
        assert_eq!(quota.windows[0].label, "5 小时（推定）");
        assert_eq!(quota.windows[1].label, "7 天");
        let legacy = parse(
            Adapter::Glm(false),
            &json!({"data":{"limits":[{"type":"TOKENS_LIMIT","percentage":0}]}}),
        )
        .unwrap();
        assert_eq!(legacy.windows.len(), 1);
        assert_eq!(legacy.windows[0].used_percent, Some(0.0));
    }
    #[test]
    fn negative_balance_is_preserved_and_malformed_data_fails() {
        let quota = parse(
            Adapter::DeepSeek,
            &json!({"balance_infos":[{"currency":"CNY","total_balance":"-0.10"}]}),
        )
        .unwrap();
        assert_eq!(quota.balances[0].amount, -0.1);
        for adapter in [
            Adapter::Kimi,
            Adapter::Glm(false),
            Adapter::MiniMax(false),
            Adapter::DeepSeek,
        ] {
            assert!(parse(adapter, &json!({})).is_err());
        }
        assert!(parse(
            Adapter::Glm(false),
            &json!({"data":{"limits":[{"type":"TOKENS_LIMIT","percentage":123}]}})
        )
        .is_err());
    }
    #[test]
    fn adapters_reject_lookalike_and_proxy_hosts() {
        let mut provider: ProviderConfig = serde_json::from_value(
            json!({"id":"test","name":"test","baseUrl":"","authTokenRef":"test"}),
        )
        .unwrap();
        for url in [
            "https://api.kimi.com.evil.test/coding",
            "http://api.kimi.com/coding",
            "https://api.kimi.com:8443/coding",
            "https://user:secret@api.kimi.com/coding",
            "https://proxy.test/anthropic",
        ] {
            provider.base_url = url.into();
            assert!(adapter(&provider).is_none());
        }
        provider.base_url = "https://api.kimi.com/coding/".into();
        assert_eq!(adapter(&provider), Some(Adapter::Kimi));
    }
}
