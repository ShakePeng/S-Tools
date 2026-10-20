use std::collections::HashMap;

const SERVICE: &str = "cn.shakepeng.stools";

fn entry(provider_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &format!("provider:{provider_id}"))
        .map_err(|error| format!("访问系统凭据库失败：{error}"))
}

pub fn read_bundle(provider_id: &str) -> Result<HashMap<String, String>, String> {
    let entry = entry(provider_id)?;
    match entry.get_password() {
        Ok(value) => {
            serde_json::from_str(&value).map_err(|error| format!("解析系统凭据失败：{error}"))
        }
        Err(keyring::Error::NoEntry) => Ok(HashMap::new()),
        Err(error) => Err(format!("读取系统凭据失败：{error}")),
    }
}

pub fn write_bundle(
    provider_id: &str,
    updates: &HashMap<String, String>,
) -> Result<HashMap<String, String>, String> {
    let mut bundle = read_bundle(provider_id)?;
    for (key, value) in updates {
        if !value.trim().is_empty() {
            bundle.insert(key.clone(), value.trim().to_string());
        }
    }
    let entry = entry(provider_id)?;
    let serialized =
        serde_json::to_string(&bundle).map_err(|error| format!("序列化系统凭据失败：{error}"))?;
    entry
        .set_password(&serialized)
        .map_err(|error| format!("写入系统凭据失败：{error}"))?;
    Ok(bundle)
}

pub fn is_configured(provider_id: &str) -> bool {
    read_bundle(provider_id)
        .map(|bundle| !bundle.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_credential_updates_are_ignored() {
        let mut updates: HashMap<String, String> = HashMap::new();
        updates.insert("apiKey".into(), "   ".into());
        assert!(updates.values().all(|value| value.trim().is_empty()));
    }
}
