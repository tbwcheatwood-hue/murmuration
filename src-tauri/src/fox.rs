use serde::Serialize;
use serde_json::{json, Value};

const LM_STUDIO_CHAT: &str = "http://127.0.0.1:1234/v1/chat/completions";
const LM_STUDIO_MODELS: &str = "http://127.0.0.1:1234/v1/models";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoxStatus {
    pub manager: String,
    pub endpoint: String,
    pub online: bool,
    pub models: Vec<String>,
    pub message: String,
}

pub async fn status() -> FoxStatus {
    let client = match reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build() {
        Ok(client) => client,
        Err(err) => {
            return FoxStatus {
                manager: "lmstudio".into(),
                endpoint: LM_STUDIO_MODELS.into(),
                online: false,
                models: Vec::new(),
                message: err.to_string(),
            }
        }
    };
    match client.get(LM_STUDIO_MODELS).send().await {
        Ok(response) if response.status().is_success() => {
            let body = response.json::<Value>().await.unwrap_or(json!({}));
            let models = body
                .get("data")
                .and_then(|value| value.as_array())
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| row.get("id").and_then(|value| value.as_str()).map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let count = models.len();
            FoxStatus {
                manager: "lmstudio".into(),
                endpoint: "http://127.0.0.1:1234/v1".into(),
                online: true,
                models,
                message: format!("LM Studio is managing {count} model id(s) on port 1234."),
            }
        }
        Ok(response) => FoxStatus {
            manager: "lmstudio".into(),
            endpoint: "http://127.0.0.1:1234/v1".into(),
            online: false,
            models: Vec::new(),
            message: format!("LM Studio answered HTTP {}.", response.status()),
        },
        Err(err) => FoxStatus {
            manager: "lmstudio".into(),
            endpoint: "http://127.0.0.1:1234/v1".into(),
            online: false,
            models: Vec::new(),
            message: format!("LM Studio is not accepting connections on 127.0.0.1:1234. {err}"),
        },
    }
}

pub async fn chat(prompt: &str, model: &str, temperature: f64, max_tokens: u32, profile: &str, tools: &[String]) -> Result<String, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Chat input is empty.".into());
    }
    let wait = (180 + max_tokens as u64 / 4).clamp(180, 600);
    let client = reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(wait))
        .build()
        .map_err(|err| err.to_string())?;
    let model = if model.trim().is_empty() {
        let report = status().await;
        report.models.into_iter().next().ok_or_else(|| "Load a model in LM Studio before sending chat.".to_string())?
    } else {
        model.trim().to_string()
    };
    let profile_line = match profile {
        "power" => "Profile: Power. Help with coding and engineering.",
        "vision" => "Profile: Vision. Describe images only when the operator includes them. LM Studio must already have a vision model loaded.",
        _ => "Profile: Safe. Help with chat and reasoning.",
    };
    let tools_line = if tools.is_empty() {
        "No T.I.M. tools are enabled on this node.".to_string()
    } else {
        format!("Enabled T.I.M. tools: {}.", tools.join(", "))
    };
    let system = format!(
        "You are FOX, the local advisor inside Murmuration. LM Studio at 127.0.0.1:1234 manages the model. You do not load, unload, or switch models. {profile_line} Advisor only: do not claim you changed files, ran commands, or moved data. {tools_line}"
    );
    let body = json!({
        "model": model,
        "temperature": temperature.clamp(0.0, 2.0),
        "max_tokens": max_tokens.clamp(1, 4096),
        "stream": false,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": prompt }
        ]
    });
    let response = client
        .post(LM_STUDIO_CHAT)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("LM Studio did not accept the chat. {}", explain(&err)))?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("LM Studio chat returned HTTP {status}. {}", clip(&text, 400)));
    }
    let payload: Value = response.json().await.map_err(|err| err.to_string())?;
    assistant_text(&payload).ok_or_else(|| "LM Studio returned a chat completion without message text.".into())
}

fn assistant_text(payload: &Value) -> Option<String> {
    let message = payload.pointer("/choices/0/message")?;
    let content = message.get("content").and_then(|value| value.as_str()).unwrap_or("").trim();
    if !content.is_empty() {
        return Some(content.to_string());
    }
    let reasoning = message.get("reasoning_content").and_then(|value| value.as_str()).unwrap_or("").trim();
    if reasoning.is_empty() { None } else { Some(reasoning.to_string()) }
}

fn explain(err: &reqwest::Error) -> String {
    let mut detail = err.to_string();
    let mut source = std::error::Error::source(err);
    while let Some(next) = source {
        detail.push_str(": ");
        detail.push_str(&next.to_string());
        source = std::error::Error::source(next);
    }
    detail
}

fn clip(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

#[tauri::command]
pub async fn fox_status() -> Result<FoxStatus, String> {
    Ok(status().await)
}

#[tauri::command]
pub async fn fox_chat(prompt: String, model: String, temperature: f64, max_tokens: u32, profile: String, tools: Vec<String>) -> Result<String, String> {
    chat(&prompt, &model, temperature, max_tokens, &profile, &tools).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_models_still_produce_chat_output() {
        let payload = json!({
            "choices": [{
                "message": { "content": "", "reasoning_content": "Hello from the loaded model." }
            }]
        });
        assert_eq!(assistant_text(&payload).as_deref(), Some("Hello from the loaded model."));
    }

    #[test]
    fn fox_chat_endpoint_is_lm_studio() {
        assert!(LM_STUDIO_CHAT.starts_with("http://127.0.0.1:1234/"));
        assert!(!LM_STUDIO_CHAT.contains("8845"));
        assert!(!LM_STUDIO_CHAT.contains("5001"));
    }
}
