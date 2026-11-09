use std::collections::HashSet;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};

use crate::providers::ProviderConfig;

const DEFAULT_TIMEOUT_SECS: u64 = 8;
const MIN_TIMEOUT_SECS: u64 = 2;
const MAX_TIMEOUT_SECS: u64 = 30;
const MODEL_FETCH_TIMEOUT_SECS: u64 = 15;
const ERROR_BODY_LIMIT: usize = 512;
const COMPAT_SUFFIXES: &[&str] = &[
    "/api/claudecode",
    "/api/anthropic",
    "/apps/anthropic",
    "/api/coding",
    "/claudecode",
    "/anthropic",
    "/step_plan",
    "/coding",
    "/claude",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointProbe {
    pub url: String,
    pub latency_ms: Option<u128>,
    pub status: Option<u16>,
    pub reachable: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FetchedModel {
    pub id: String,
    pub owned_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    owned_by: Option<String>,
}

pub async fn test_endpoints(
    provider: &ProviderConfig,
    timeout_secs: Option<u64>,
) -> Result<Vec<EndpointProbe>, String> {
    let timeout = Duration::from_secs(sanitize_timeout(timeout_secs));
    let client = client(timeout, provider)?;
    let mut urls = vec![provider.base_url.clone()];
    urls.extend(provider.meta.endpoint_candidates.clone());
    let mut seen = HashSet::new();
    urls.retain(|url| {
        let normalized = url.trim().trim_end_matches('/').to_string();
        !normalized.is_empty() && seen.insert(normalized)
    });

    let mut probes = Vec::with_capacity(urls.len());
    for url in urls {
        let url = url.trim().to_string();
        if reqwest::Url::parse(&url).is_err() {
            probes.push(EndpointProbe {
                url,
                latency_ms: None,
                status: None,
                reachable: false,
                error: Some("URL 无效".into()),
            });
            continue;
        }
        let started = Instant::now();
        match client.get(&url).send().await {
            Ok(response) => probes.push(EndpointProbe {
                url,
                latency_ms: Some(started.elapsed().as_millis()),
                status: Some(response.status().as_u16()),
                reachable: true,
                error: None,
            }),
            Err(error) => probes.push(EndpointProbe {
                url,
                latency_ms: None,
                status: error.status().map(|status| status.as_u16()),
                reachable: false,
                error: Some(if error.is_timeout() {
                    "请求超时".into()
                } else if error.is_connect() {
                    "连接失败".into()
                } else {
                    error.to_string()
                }),
            }),
        }
    }
    Ok(probes)
}

pub async fn fetch_models(
    provider: &ProviderConfig,
    api_key: &str,
) -> Result<Vec<FetchedModel>, String> {
    if api_key.trim().is_empty() {
        return Err("请先填写 API Key".into());
    }
    let client = client(Duration::from_secs(MODEL_FETCH_TIMEOUT_SECS), provider)?;
    let candidates = build_model_urls(provider)?;
    let mut last_error = String::new();

    for url in candidates {
        let mut request = client.get(&url);
        request = if provider.meta.api_key_field == "ANTHROPIC_API_KEY" {
            request.header("x-api-key", api_key)
        } else {
            request.header(AUTHORIZATION, format!("Bearer {api_key}"))
        };
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                let response: ModelsResponse = response
                    .json()
                    .await
                    .map_err(|error| format!("模型列表响应不是有效 JSON：{error}"))?;
                let mut models = response
                    .data
                    .into_iter()
                    .filter(|model| !model.id.trim().is_empty())
                    .map(|model| FetchedModel {
                        id: model.id,
                        owned_by: model.owned_by,
                    })
                    .collect::<Vec<_>>();
                models.sort_by(|left, right| left.id.cmp(&right.id));
                models.dedup_by(|left, right| left.id == right.id);
                return Ok(models);
            }
            Ok(response) => {
                let status = response.status();
                let body = truncate_body(response.text().await.unwrap_or_default());
                last_error = format!("{url} 返回 HTTP {status}：{body}");
                if status.as_u16() != 404 && status.as_u16() != 405 {
                    return Err(last_error);
                }
            }
            Err(error) => return Err(format!("请求模型列表失败：{error}")),
        }
    }
    Err(if last_error.is_empty() {
        "没有可用的模型列表地址".into()
    } else {
        last_error
    })
}

fn client(timeout: Duration, provider: &ProviderConfig) -> Result<reqwest::Client, String> {
    let mut headers = HeaderMap::new();
    for (name, value) in &provider.meta.request_headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| format!("请求头名称无效：{error}"))?;
        let value =
            HeaderValue::from_str(value).map_err(|error| format!("请求头值无效：{error}"))?;
        headers.insert(name, value);
    }
    if !provider.meta.custom_user_agent.is_empty() {
        let value = HeaderValue::from_str(&provider.meta.custom_user_agent)
            .map_err(|error| format!("User-Agent 无效：{error}"))?;
        headers.insert(USER_AGENT, value);
    }
    crate::general_settings::configure_http_client(reqwest::Client::builder())
        .timeout(timeout)
        .default_headers(headers)
        .build()
        .map_err(|error| format!("创建网络客户端失败：{error}"))
}

pub fn build_model_urls(provider: &ProviderConfig) -> Result<Vec<String>, String> {
    if !provider.meta.models_url.trim().is_empty() {
        return Ok(vec![provider.meta.models_url.trim().to_string()]);
    }
    let base = provider.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("API 端点不能为空".into());
    }
    let mut urls = Vec::new();
    if provider.meta.is_full_url {
        let parsed = reqwest::Url::parse(base).map_err(|_| "完整 URL 无效".to_string())?;
        let origin = parsed.origin().ascii_serialization();
        urls.push(format!("{origin}/v1/models"));
    } else if ends_with_version_segment(base) {
        urls.push(format!("{base}/models"));
        if !base.ends_with("/v1") {
            urls.push(format!("{base}/v1/models"));
        }
    } else {
        urls.push(format!("{base}/v1/models"));
    }
    if let Some(root) = strip_compat_suffix(base) {
        urls.push(format!("{root}/v1/models"));
        urls.push(format!("{root}/models"));
    }
    let mut seen = HashSet::new();
    urls.retain(|url| seen.insert(url.clone()));
    Ok(urls)
}

fn sanitize_timeout(value: Option<u64>) -> u64 {
    value
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
}

fn ends_with_version_segment(url: &str) -> bool {
    url.rsplit('/').next().is_some_and(|segment| {
        segment.strip_prefix('v').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|value| value.is_ascii_digit())
        })
    })
}

fn strip_compat_suffix(url: &str) -> Option<&str> {
    COMPAT_SUFFIXES
        .iter()
        .find_map(|suffix| url.strip_suffix(suffix))
        .map(|value| value.trim_end_matches('/'))
}

fn truncate_body(body: String) -> String {
    if body.chars().count() <= ERROR_BODY_LIMIT {
        body
    } else {
        let mut output = body.chars().take(ERROR_BODY_LIMIT).collect::<String>();
        output.push('…');
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ProviderMeta;
    use std::collections::HashMap;

    fn provider(base_url: &str) -> ProviderConfig {
        ProviderConfig {
            id: "test".into(),
            name: "Test".into(),
            base_url: base_url.into(),
            auth_token_ref: "test".into(),
            env: HashMap::new(),
            timeout_ms: None,
            disable_nonessential_traffic: false,
            builtin: false,
            hidden: false,
            meta: ProviderMeta::default(),
        }
    }

    #[test]
    fn derives_model_urls_from_anthropic_compat_path() {
        assert_eq!(
            build_model_urls(&provider("https://api.example.com/anthropic")).unwrap(),
            vec![
                "https://api.example.com/anthropic/v1/models",
                "https://api.example.com/v1/models",
                "https://api.example.com/models",
            ]
        );
    }

    #[test]
    fn explicit_models_url_has_priority() {
        let mut value = provider("https://api.example.com/anthropic");
        value.meta.models_url = "https://models.example.com/list".into();
        assert_eq!(
            build_model_urls(&value).unwrap(),
            vec!["https://models.example.com/list"]
        );
    }

    #[test]
    fn timeout_is_clamped() {
        assert_eq!(sanitize_timeout(Some(1)), MIN_TIMEOUT_SECS);
        assert_eq!(sanitize_timeout(Some(99)), MAX_TIMEOUT_SECS);
        assert_eq!(sanitize_timeout(None), DEFAULT_TIMEOUT_SECS);
    }
}
