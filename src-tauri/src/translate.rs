use std::{collections::HashMap, time::Instant};

use chrono::Utc;
use hmac::{Hmac, Mac};
use md5::{Digest as Md5Digest, Md5};
use serde_json::{json, Value};
use sha2::Sha256;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::models::{
    ProviderConfig, ProviderInput, ProviderResult, ProviderView, TranslationRequest,
};

type HmacSha256 = Hmac<Sha256>;

pub fn provider_views(providers: &[ProviderConfig]) -> Vec<ProviderView> {
    providers
        .iter()
        .map(|provider| ProviderView {
            id: provider.id.clone(),
            name: provider.name.clone(),
            kind: provider.kind.clone(),
            enabled: provider.enabled,
            configured: provider.configured,
            model: provider.model.clone(),
            base_url: provider.base_url.clone(),
            description: provider.description.clone(),
            credentials: provider.credentials.clone(),
        })
        .collect()
}

pub fn credentials_complete(provider: &ProviderConfig) -> bool {
    validate_credentials(
        &provider.kind,
        &provider.base_url,
        &provider.model,
        &provider.credentials,
    )
    .is_ok()
}

pub fn apply_provider_input(
    config: &mut ProviderConfig,
    input: &ProviderInput,
) -> Result<(), String> {
    if input.id.trim().is_empty() {
        return Err("翻译服务 ID 不能为空".into());
    }
    if !input.base_url.trim().is_empty() {
        validate_base_url(&input.base_url)?;
    }
    let mut next = config.clone();
    next.name = input.name.trim().to_string();
    next.kind = input.kind.clone();
    next.enabled = input.enabled;
    next.model = input.model.trim().to_string();
    next.base_url = input.base_url.trim().trim_end_matches('/').to_string();
    if next.kind == "deepseek" {
        if next.base_url.is_empty() {
            next.base_url = "https://api.deepseek.com".into();
        }
        if next.model.is_empty() {
            next.model = "deepseek-chat".into();
        }
    }
    merge_credentials(&mut next.credentials, &input.credentials);
    next.configured = credentials_complete(&next);
    if next.enabled {
        validate_credentials(&next.kind, &next.base_url, &next.model, &next.credentials)?;
    }
    *config = next;
    Ok(())
}

fn merge_credentials(existing: &mut HashMap<String, String>, updates: &HashMap<String, String>) {
    for (key, value) in updates {
        if !value.trim().is_empty() {
            existing.insert(key.clone(), value.trim().to_string());
        }
    }
}

fn validate_credentials(
    kind: &str,
    base_url: &str,
    model: &str,
    credentials: &HashMap<String, String>,
) -> Result<(), String> {
    let require = |key: &str, label: &str| -> Result<(), String> {
        credentials
            .get(key)
            .filter(|value| !value.trim().is_empty())
            .map(|_| ())
            .ok_or_else(|| format!("启用服务前需要填写{label}"))
    };
    match kind {
        "baidu" => {
            require("appId", "App ID")?;
            require("secret", "App 密钥")?;
        }
        "google" => require("apiKey", "Google API Key")?,
        "tencent" => {
            require("secretId", "Secret ID")?;
            require("secretKey", "Secret Key")?;
        }
        "deepseek" | "openai-compatible" => {
            require("apiKey", "API Key")?;
            if base_url.trim().is_empty() {
                return Err("启用服务前需要填写 Base URL".into());
            }
            if model.trim().is_empty() {
                return Err("启用服务前需要填写模型名称".into());
            }
        }
        _ => return Err("不支持的翻译服务类型".into()),
    }
    Ok(())
}

fn validate_base_url(value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|error| format!("Base URL 无效：{error}"))?;
    if url.host_str().unwrap_or_default().is_empty() {
        return Err("Base URL 缺少主机名".into());
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Base URL 只支持 HTTP 或 HTTPS".into());
    }
    Ok(())
}

pub async fn perform_translation(
    client: &reqwest::Client,
    provider: &ProviderConfig,
    request: &TranslationRequest,
    cancellation: CancellationToken,
) -> Result<ProviderResult, String> {
    if request.text.trim().is_empty() {
        return Err("没有可翻译的文本".into());
    }
    let credentials = &provider.credentials;
    let started = Instant::now();
    let (translated, detected) = tokio::select! {
        _ = cancellation.cancelled() => return Err("翻译已取消".into()),
        result = async {
            match provider.kind.as_str() {
                "baidu" => translate_baidu(client, provider, credentials, request).await,
                "google" => translate_google(client, credentials, request).await,
                "tencent" => translate_tencent(client, credentials, request).await,
                "deepseek" | "openai-compatible" => {
                    translate_openai(client, provider, credentials, request).await
                }
                _ => Err("不支持的翻译服务类型".to_string()),
            }
        } => result?,
    };
    Ok(ProviderResult {
        request_id: request.request_id.clone(),
        provider_id: provider.id.clone(),
        translated_text: translated,
        detected_lang: detected.unwrap_or_else(|| request.source_lang.clone()),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

async fn translate_baidu(
    client: &reqwest::Client,
    _provider: &ProviderConfig,
    credentials: &HashMap<String, String>,
    request: &TranslationRequest,
) -> Result<(String, Option<String>), String> {
    let app_id = credential(credentials, "appId")?;
    let secret = credential(credentials, "secret")?;
    let salt = Utc::now().timestamp_millis().to_string();
    let sign_source = format!("{app_id}{}{salt}{secret}", request.text);
    let sign = hex::encode(Md5::digest(sign_source.as_bytes()));
    let target = match request.target_lang.as_str() {
        "zh-TW" => "cht",
        "zh-CN" => "zh",
        "en" => "en",
        "ja" => "jp",
        "ko" => "kor",
        other => other,
    };
    let source = match request.source_lang.as_str() {
        "auto" => "auto",
        "zh-CN" | "zh-TW" => "zh",
        "en" => "en",
        "ja" => "jp",
        "ko" => "kor",
        other => other,
    };
    let form = [
        ("q", request.text.as_str()),
        ("from", source),
        ("to", target),
        ("appid", app_id),
        ("salt", salt.as_str()),
        ("sign", sign.as_str()),
    ];
    let value = post_form_json(
        client,
        "https://fanyi-api.baidu.com/api/trans/vip/translate",
        &form,
    )
    .await?;
    if let Some(code) = value.get("error_code").and_then(Value::as_str) {
        return Err(format!(
            "百度翻译失败：{} ({code})",
            value
                .get("error_msg")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
        ));
    }
    let segments = value
        .get("trans_result")
        .and_then(Value::as_array)
        .ok_or_else(|| "百度翻译返回结果缺少 trans_result".to_string())?;
    let translated = segments
        .iter()
        .filter_map(|segment| segment.get("dst").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let detected = segments
        .first()
        .and_then(|segment| segment.get("from").and_then(Value::as_str))
        .map(ToOwned::to_owned);
    Ok((translated, detected))
}

async fn translate_google(
    client: &reqwest::Client,
    credentials: &HashMap<String, String>,
    request: &TranslationRequest,
) -> Result<(String, Option<String>), String> {
    let api_key = credential(credentials, "apiKey")?;
    let endpoint = format!(
        "https://translation.googleapis.com/language/translate/v2?key={}",
        urlencoding(api_key)
    );
    let target = google_language(&request.target_lang);
    let mut body = json!({
        "q": [request.text],
        "target": target,
        "format": "text"
    });
    if request.source_lang != "auto" {
        body["source"] = Value::String(google_language(&request.source_lang).into());
    }
    let response = client
        .post(endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Google 翻译请求失败：{error}"))?;
    let value = decode_response(response, "Google 翻译").await?;
    let translation = value
        .pointer("/data/translations/0")
        .ok_or_else(|| "Google 翻译返回结果格式异常".to_string())?;
    let text = translation
        .get("translatedText")
        .and_then(Value::as_str)
        .ok_or_else(|| "Google 翻译返回结果缺少 translatedText".to_string())?;
    Ok((
        decode_html_entities(text),
        translation
            .get("detectedSourceLanguage")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    ))
}

async fn translate_tencent(
    client: &reqwest::Client,
    credentials: &HashMap<String, String>,
    request: &TranslationRequest,
) -> Result<(String, Option<String>), String> {
    let secret_id = credential(credentials, "secretId")?;
    let secret_key = credential(credentials, "secretKey")?;
    let timestamp = Utc::now().timestamp();
    let host = "tmt.tencentcloudapi.com";
    let payload = serde_json::to_string(&json!({
        "SourceText": request.text,
        "Source": tencent_language(&request.source_lang),
        "Target": tencent_language(&request.target_lang),
        "ProjectId": 0
    }))
    .map_err(|error| format!("构造腾讯云请求失败：{error}"))?;
    let authorization = tencent_authorization(secret_id, secret_key, timestamp, host, &payload);
    let response = client
        .post(format!("https://{host}/"))
        .header("Authorization", authorization)
        .header("Content-Type", "application/json; charset=utf-8")
        .header("Host", host)
        .header("X-TC-Action", "TextTranslate")
        .header("X-TC-Version", "2018-03-21")
        .header("X-TC-Timestamp", timestamp.to_string())
        .header("X-TC-Region", "ap-shanghai")
        .body(payload)
        .send()
        .await
        .map_err(|error| format!("腾讯云翻译请求失败：{error}"))?;
    let value = decode_response(response, "腾讯云翻译").await?;
    if let Some(error) = value
        .pointer("/Response/Error/Message")
        .and_then(Value::as_str)
    {
        return Err(format!("腾讯云翻译失败：{error}"));
    }
    let response = value
        .get("Response")
        .ok_or_else(|| "腾讯云翻译返回结果缺少 Response".to_string())?;
    let text = response
        .get("TargetText")
        .and_then(Value::as_str)
        .ok_or_else(|| "腾讯云翻译返回结果缺少 TargetText".to_string())?;
    Ok((
        text.to_string(),
        response
            .get("Source")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    ))
}

async fn translate_openai(
    client: &reqwest::Client,
    provider: &ProviderConfig,
    credentials: &HashMap<String, String>,
    request: &TranslationRequest,
) -> Result<(String, Option<String>), String> {
    let api_key = credential(credentials, "apiKey")?;
    let base_url = provider.base_url.trim_end_matches('/');
    let endpoint = format!("{base_url}/chat/completions");
    let target_name = language_display_name(&request.target_lang);
    let source_instruction = if request.source_lang == "auto" {
        "自动识别源语言".to_string()
    } else {
        format!("源语言为{}", language_display_name(&request.source_lang))
    };
    let body = json!({
        "model": provider.model,
        "temperature": 0.1,
        "messages": [
            {
                "role": "system",
                "content": "你是专业翻译引擎。只输出译文，不要解释、不要添加引号或 Markdown 围栏。保留原文段落、换行和专有名词。"
            },
            {
                "role": "user",
                "content": format!("{source_instruction}。请翻译为{target_name}：\n\n{}", request.text)
            }
        ]
    });
    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("{} 请求失败：{error}", provider.name))?;
    let value = decode_response(response, &provider.name).await?;
    if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
        return Err(format!("{} 返回错误：{message}", provider.name));
    }
    let text = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{} 返回结果缺少译文", provider.name))?;
    Ok((text.to_string(), None))
}

async fn post_form_json(
    client: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<Value, String> {
    let response = client
        .post(url)
        .form(form)
        .send()
        .await
        .map_err(|error| format!("翻译请求失败：{error}"))?;
    decode_response(response, "百度翻译").await
}

async fn decode_response(response: reqwest::Response, service: &str) -> Result<Value, String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("{service} 响应读取失败：{error}"))?;
    if !status.is_success() {
        return Err(format!(
            "{service} 请求失败：HTTP {} {}",
            status.as_u16(),
            truncate_error(&text)
        ));
    }
    serde_json::from_str(&text)
        .map_err(|error| format!("{service} 响应解析失败：{error}；{}", truncate_error(&text)))
}

fn truncate_error(value: &str) -> String {
    let clean = value.replace(['\r', '\n'], " ");
    clean.chars().take(320).collect()
}

fn credential<'a>(credentials: &'a HashMap<String, String>, key: &str) -> Result<&'a str, String> {
    credentials
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("翻译服务缺少凭据：{key}"))
}

fn google_language(language: &str) -> &str {
    match language {
        "auto" => "auto",
        "zh-CN" => "zh-CN",
        "zh-TW" => "zh-TW",
        "en" => "en",
        "ja" => "ja",
        "ko" => "ko",
        other => other,
    }
}

fn tencent_language(language: &str) -> &str {
    match language {
        "auto" => "auto",
        "zh-CN" => "zh",
        "zh-TW" => "zh-TW",
        "en" => "en",
        "ja" => "ja",
        "ko" => "ko",
        other => other,
    }
}

fn language_display_name(language: &str) -> &str {
    match language {
        "zh-CN" => "简体中文",
        "zh-TW" => "繁体中文",
        "en" => "英语",
        "ja" => "日语",
        "ko" => "韩语",
        _ => language,
    }
}

fn urlencoding(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn tencent_authorization(
    secret_id: &str,
    secret_key: &str,
    timestamp: i64,
    host: &str,
    payload: &str,
) -> String {
    let date = chrono::DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(Utc::now)
        .format("%Y-%m-%d")
        .to_string();
    let hashed_payload = hex::encode(Sha256::digest(payload.as_bytes()));
    let canonical_headers =
        "content-type:application/json; charset=utf-8\nhost:tmt.tencentcloudapi.com\n";
    let signed_headers = "content-type;host";
    let canonical_request =
        format!("POST\n/\n\n{canonical_headers}\n{signed_headers}\n{hashed_payload}");
    let credential_scope = format!("{date}/tmt/tc3_request");
    let string_to_sign = format!(
        "TC3-HMAC-SHA256\n{timestamp}\n{credential_scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );
    let secret_date = hmac_sha256(format!("TC3{secret_key}").as_bytes(), date.as_bytes());
    let secret_service = hmac_sha256(&secret_date, b"tmt");
    let secret_signing = hmac_sha256(&secret_service, b"tc3_request");
    let signature = hex::encode(hmac_sha256(&secret_signing, string_to_sign.as_bytes()));
    let _ = host;
    format!(
        "TC3-HMAC-SHA256 Credential={secret_id}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts arbitrary key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tencent_signature_is_deterministic() {
        let first = tencent_authorization(
            "AKIDEXAMPLE",
            "SECRET",
            1_700_000_000,
            "tmt.tencentcloudapi.com",
            r#"{"SourceText":"hello"}"#,
        );
        let second = tencent_authorization(
            "AKIDEXAMPLE",
            "SECRET",
            1_700_000_000,
            "tmt.tencentcloudapi.com",
            r#"{"SourceText":"hello"}"#,
        );
        assert_eq!(first, second);
        assert!(first.contains("Credential=AKIDEXAMPLE/2023-11-14/tmt/tc3_request"));
    }

    #[test]
    fn language_mapping_is_stable() {
        assert_eq!(tencent_language("zh-TW"), "zh-TW");
        assert_eq!(google_language("zh-CN"), "zh-CN");
        assert_eq!(language_display_name("en"), "英语");
    }

    #[test]
    fn html_entities_are_decoded() {
        assert_eq!(
            decode_html_entities("Tom &amp; Jerry &#39;show&#39;"),
            "Tom & Jerry 'show'"
        );
    }

    #[test]
    fn base_url_accepts_http_and_https() {
        assert!(validate_base_url("http://192.168.1.20:8000/v1").is_ok());
        assert!(validate_base_url("https://api.example.com/v1").is_ok());
        assert!(validate_base_url("ftp://api.example.com/v1").is_err());
    }

    #[test]
    fn provider_credentials_are_saved_and_preserved_when_blank() {
        let mut provider = crate::models::AppConfig::default()
            .providers
            .into_iter()
            .find(|provider| provider.id == "openai")
            .expect("openai provider");
        let mut credentials = HashMap::new();
        credentials.insert("apiKey".into(), "secret-key".into());
        let input = ProviderInput {
            id: provider.id.clone(),
            name: provider.name.clone(),
            kind: provider.kind.clone(),
            enabled: true,
            model: "deepseek-v4.1-flash".into(),
            base_url: "http://127.0.0.1:7864/v1".into(),
            credentials,
        };
        apply_provider_input(&mut provider, &input).expect("valid provider");
        assert_eq!(provider.credentials["apiKey"], "secret-key");
        assert!(provider.configured);

        let input = ProviderInput {
            credentials: HashMap::new(),
            ..input
        };
        apply_provider_input(&mut provider, &input).expect("preserve credentials");
        assert_eq!(provider.credentials["apiKey"], "secret-key");
    }

    #[test]
    fn provider_views_include_saved_credentials_for_settings() {
        let mut config = crate::models::AppConfig::default();
        config.providers[4]
            .credentials
            .insert("apiKey".into(), "visible-key".into());

        let views = provider_views(&config.providers);
        assert_eq!(views[4].credentials["apiKey"], "visible-key");
    }
}
