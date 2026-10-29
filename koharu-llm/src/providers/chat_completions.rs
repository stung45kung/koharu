use std::sync::Arc;

use reqwest_middleware::ClientWithMiddleware;
use serde::Serialize;

use super::ensure_provider_success;

pub enum ChatCompletionsAuth {
    None,
    Bearer(String),
}

pub struct ChatCompletionsRequest {
    pub provider: &'static str,
    pub endpoint: String,
    pub auth: ChatCompletionsAuth,
    pub model: String,
    pub system_prompt: String,
    pub user_prompt: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

pub async fn send_chat_completion(
    http_client: Arc<ClientWithMiddleware>,
    request: ChatCompletionsRequest,
) -> anyhow::Result<String> {
    let body = ChatRequest {
        model: &request.model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: request.system_prompt,
            },
            ChatMessage {
                role: "user",
                content: request.user_prompt,
            },
        ],
        temperature: request.temperature,
        max_tokens: request.max_tokens,
    };

    let mut http_request = http_client.post(&request.endpoint);
    if let ChatCompletionsAuth::Bearer(api_key) = request.auth {
        http_request = http_request.bearer_auth(api_key);
    }

    let response = http_request
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&body)?)
        .send()
        .await?;

    let resp: serde_json::Value = ensure_provider_success(request.provider, response)
        .await?
        .json()
        .await?;

    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("{} returned no content", request.provider))
}

/// One user message with an instruction and a PNG image (OpenAI-style
/// `image_url` content part, accepted by LM Studio and other servers).
pub struct VisionRequest {
    pub provider: &'static str,
    pub endpoint: String,
    pub auth: ChatCompletionsAuth,
    pub model: String,
    pub instructions: String,
    pub png: Vec<u8>,
    pub max_tokens: u32,
}

pub async fn send_vision_completion(
    http_client: Arc<ClientWithMiddleware>,
    request: VisionRequest,
) -> anyhow::Result<String> {
    use base64::Engine as _;
    let image_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&request.png)
    );
    let body = serde_json::json!({
        "model": request.model,
        "temperature": 0.0,
        "max_tokens": request.max_tokens,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": request.instructions },
                { "type": "image_url", "image_url": { "url": image_url } },
            ],
        }],
    });

    let mut http_request = http_client.post(&request.endpoint);
    if let ChatCompletionsAuth::Bearer(api_key) = request.auth {
        http_request = http_request.bearer_auth(api_key);
    }
    let response = http_request
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&body)?)
        .send()
        .await?;
    let resp: serde_json::Value = ensure_provider_success(request.provider, response)
        .await?
        .json()
        .await?;
    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("{} returned no content", request.provider))
}
