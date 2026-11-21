use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const MODEL: &str = "murmuration";
const API_KEY: &str = "local";
const PROVIDER_ID: &str = "murmuration";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeClient {
    pub id: String,
    pub name: String,
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub path: String,
    pub message: String,
}

pub fn gateway_base(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

pub fn cursor_settings_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            return PathBuf::from(appdata).join("Cursor").join("User").join("settings.json");
        }
    }
    if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support").join("Cursor").join("User").join("settings.json")
    } else {
        home().join(".config").join("Cursor").join("User").join("settings.json")
    }
}

pub fn zcode_provider_path() -> PathBuf {
    home().join(".zcode").join("v2").join("provider_config.json")
}

fn home() -> PathBuf {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.is_empty() {
            return PathBuf::from(profile);
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

pub fn client_status(id: &str, port: u16) -> Result<IdeClient, String> {
    let base = gateway_base(port);
    match id {
        "cursor" => Ok(describe("cursor", "Cursor", &cursor_settings_path(), cursor_configured(&cursor_settings_path(), &base)?, &base)),
        "zcode" => Ok(describe("zcode", "Zcode", &zcode_provider_path(), zcode_configured(&zcode_provider_path(), &base)?, &base)),
        other => Err(format!("Unknown IDE client {other}")),
    }
}

pub fn apply_client(id: &str, port: u16) -> Result<IdeClient, String> {
    let base = gateway_base(port);
    match id {
        "cursor" => {
            let path = cursor_settings_path();
            write_cursor(&path, &base)?;
            Ok(describe("cursor", "Cursor", &path, true, &base))
        }
        "zcode" => {
            let path = zcode_provider_path();
            write_zcode(&path, &base)?;
            Ok(describe("zcode", "Zcode", &path, true, &base))
        }
        other => Err(format!("Unknown IDE client {other}")),
    }
}

fn describe(id: &str, name: &str, path: &Path, configured: bool, base: &str) -> IdeClient {
    let message = if configured {
        format!("{name} is pointed at this gateway. The model id is {MODEL}.")
    } else if path.exists() {
        format!("{name} is installed, and it is not pointed at this gateway yet.")
    } else {
        format!("{name} settings are not on this machine yet. Apply creates them.")
    };
    IdeClient {
        id: id.into(),
        name: name.into(),
        configured,
        base_url: base.into(),
        model: MODEL.into(),
        path: path.display().to_string(),
        message,
    }
}

fn read_json(path: &Path) -> Result<Option<Value>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text).map(Some).map_err(|_| {
        format!(
            "{} is not plain JSON, so it was left unchanged.",
            path.display()
        )
    })
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("Could not create {}: {err}", parent.display()))?;
    }
    let body = serde_json::to_vec_pretty(value).map_err(|err| err.to_string())?;
    let mut bytes = body;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|err| format!("Could not write {}: {err}", path.display()))
}

fn cursor_configured(path: &Path, base: &str) -> Result<bool, String> {
    let Some(value) = read_json(path)? else {
        return Ok(false);
    };
    Ok(value.get("cursor.openai.baseUrl").and_then(|item| item.as_str()) == Some(base)
        && value.get("cursor.general.openAIBaseURL").and_then(|item| item.as_str()) == Some(base))
}

fn write_cursor(path: &Path, base: &str) -> Result<(), String> {
    let mut value = read_json(path)?.unwrap_or_else(|| json!({}));
    let object = value.as_object_mut().ok_or_else(|| format!("{} must be a JSON object.", path.display()))?;
    object.insert("cursor.openai.baseUrl".into(), json!(base));
    object.insert("cursor.general.openAIBaseURL".into(), json!(base));
    object.insert("cursor.openaiApiKey".into(), json!(API_KEY));
    object.insert("cursor.general.openAIAPIKey".into(), json!(API_KEY));
    write_json(path, &value)
}

fn zcode_configured(path: &Path, base: &str) -> Result<bool, String> {
    let Some(value) = read_json(path)? else {
        return Ok(false);
    };
    let rules = value
        .pointer("/config/providerConfigRules/providerRules")
        .and_then(|item| item.as_array());
    let Some(rules) = rules else {
        return Ok(false);
    };
    Ok(rules.iter().any(|rule| {
        rule.get("providerId").and_then(|item| item.as_str()) == Some(PROVIDER_ID)
            && rule.pointer("/config/api/type").and_then(|item| item.as_str()) == Some("openai-chat-completions")
            && rule.pointer("/config/api/baseUrl").and_then(|item| item.as_str()) == Some(base)
    }))
}

fn write_zcode(path: &Path, base: &str) -> Result<(), String> {
    let mut value = read_json(path)?.unwrap_or_else(|| {
        json!({
            "schemaVersion": 1,
            "config": {
                "providerConfigRules": { "providerRules": [] },
                "modelConfigRules": { "providerModelRules": [], "manualProviderModelRules": [] }
            }
        })
    });
    if !value.is_object() {
        return Err(format!("{} must be a JSON object.", path.display()));
    }
    ensure_zcode_arrays(&mut value);
    let provider = json!({
        "providerId": PROVIDER_ID,
        "providerName": "Murmuration",
        "enabled": true,
        "config": {
            "group": "standard-personal",
            "access": { "type": "api-key", "apiKey": API_KEY },
            "api": { "type": "openai-chat-completions", "baseUrl": base },
            "personalModelIds": [MODEL],
            "modelOrder": [MODEL]
        }
    });
    let model = json!({
        "providerId": PROVIDER_ID,
        "modelId": MODEL,
        "config": {
            "enabled": true,
            "optionSpecs": {
                "maxOutputTokens": { "max": 4096, "map": "{\"max_tokens\": maxOutputTokens}" },
                "reasoningLevel": { "map": "{}" }
            }
        }
    });
    replace_id(value.pointer_mut("/config/providerConfigRules/providerRules").unwrap(), &provider);
    replace_pair(
        value.pointer_mut("/config/modelConfigRules/providerModelRules").unwrap(),
        &model,
    );
    if let Some(manual) = value.pointer_mut("/config/modelConfigRules/manualProviderModelRules").and_then(|item| item.as_array_mut()) {
        manual.retain(|rule| !same_model(rule));
    }
    if let Some(order) = value.pointer_mut("/config/providerOrder").and_then(|item| item.as_array_mut()) {
        if !order.iter().any(|item| item.as_str() == Some(PROVIDER_ID)) {
            order.push(json!(PROVIDER_ID));
        }
    }
    write_json(path, &value)
}

fn ensure_zcode_arrays(value: &mut Value) {
    if value.get("schemaVersion").is_none() {
        value["schemaVersion"] = json!(1);
    }
    let config = value.as_object_mut().unwrap().entry("config").or_insert_with(|| json!({}));
    if !config.is_object() {
        *config = json!({});
    }
    let providers = config
        .as_object_mut()
        .unwrap()
        .entry("providerConfigRules")
        .or_insert_with(|| json!({}));
    if !providers.is_object() {
        *providers = json!({});
    }
    let rules = providers.as_object_mut().unwrap().entry("providerRules").or_insert_with(|| json!([]));
    if !rules.is_array() {
        *rules = json!([]);
    }
    let models = config.as_object_mut().unwrap().entry("modelConfigRules").or_insert_with(|| json!({}));
    if !models.is_object() {
        *models = json!({});
    }
    for key in ["providerModelRules", "manualProviderModelRules"] {
        let slot = models.as_object_mut().unwrap().entry(key).or_insert_with(|| json!([]));
        if !slot.is_array() {
            *slot = json!([]);
        }
    }
}

fn replace_id(rules: &mut Value, next: &Value) {
    let list = rules.as_array_mut().unwrap();
    list.retain(|rule| rule.get("providerId").and_then(|item| item.as_str()) != Some(PROVIDER_ID));
    list.push(next.clone());
}

fn replace_pair(rules: &mut Value, next: &Value) {
    let list = rules.as_array_mut().unwrap();
    list.retain(|rule| !same_model(rule));
    list.push(next.clone());
}

#[tauri::command]
pub fn ide_client_status(state: tauri::State<crate::state::Shared>) -> Result<Vec<IdeClient>, String> {
    let port = crate::state::read_lock(state.inner()).gateway_port;
    Ok(vec![client_status("cursor", port)?, client_status("zcode", port)?])
}

#[tauri::command]
pub fn apply_ide_client(state: tauri::State<crate::state::Shared>, client: String) -> Result<IdeClient, String> {
    let port = crate::state::read_lock(state.inner()).gateway_port;
    let applied = apply_client(&client, port)?;
    crate::state::write_lock(state.inner()).log(
        "gateway",
        "orchestrator",
        "info",
        format!("{} now uses {} as model {}.", applied.name, applied.base_url, applied.model),
    );
    Ok(applied)
}

fn same_model(rule: &Value) -> bool {
    rule.get("providerId").and_then(|item| item.as_str()) == Some(PROVIDER_ID)
        && rule.get("modelId").and_then(|item| item.as_str()) == Some(MODEL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> PathBuf {
        let tick = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("murmuration-clients-{tick}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn cursor_merge_keeps_other_settings() {
        let dir = scratch();
        let path = dir.join("settings.json");
        fs::write(&path, "{\n  \"window.autoDetectColorScheme\": true\n}\n").unwrap();
        write_cursor(&path, "http://127.0.0.1:11435/v1").unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["window.autoDetectColorScheme"], true);
        assert_eq!(value["cursor.openai.baseUrl"], "http://127.0.0.1:11435/v1");
        assert_eq!(value["cursor.general.openAIAPIKey"], "local");
        assert!(cursor_configured(&path, "http://127.0.0.1:11435/v1").unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cursor_leaves_jsonc_untouched() {
        let dir = scratch();
        let path = dir.join("settings.json");
        let original = "{\n  // keep\n  \"editor.fontSize\": 14\n}\n";
        fs::write(&path, original).unwrap();
        let err = write_cursor(&path, "http://127.0.0.1:11435/v1").unwrap_err();
        assert!(err.contains("left unchanged"));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn zcode_provider_is_upserted_and_keeps_other_providers() {
        let dir = scratch();
        let path = dir.join("provider_config.json");
        fs::write(
            &path,
            r#"{"schemaVersion":1,"config":{"providerConfigRules":{"providerRules":[{"providerId":"other","providerName":"Other"}]},"modelConfigRules":{"providerModelRules":[],"manualProviderModelRules":[{"providerId":"murmuration","modelId":"murmuration","config":{"enabled":false}}]}}}"#,
        )
        .unwrap();
        write_zcode(&path, "http://127.0.0.1:11436/v1").unwrap();
        write_zcode(&path, "http://127.0.0.1:11436/v1").unwrap();
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let rules = value.pointer("/config/providerConfigRules/providerRules").unwrap().as_array().unwrap();
        assert_eq!(rules.iter().filter(|rule| rule["providerId"] == "murmuration").count(), 1);
        assert_eq!(rules.iter().filter(|rule| rule["providerId"] == "other").count(), 1);
        assert_eq!(rules.iter().find(|rule| rule["providerId"] == "murmuration").unwrap()["config"]["api"]["baseUrl"], "http://127.0.0.1:11436/v1");
        let manual = value.pointer("/config/modelConfigRules/manualProviderModelRules").unwrap().as_array().unwrap();
        assert!(manual.is_empty());
        assert!(zcode_configured(&path, "http://127.0.0.1:11436/v1").unwrap());
        let _ = fs::remove_dir_all(dir);
    }
}
