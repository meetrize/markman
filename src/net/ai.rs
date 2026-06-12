//! OpenAI-compatible chat completions client used by editor AI actions.

use anyhow::{Context as _, bail};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::config::AiPreferences;

const DEFAULT_SYSTEM_PROMPT: &str = "You are an assistant embedded in a Markdown notes editor. Return concise Markdown only. Do not wrap the whole answer in a code block unless the user asks for code.";

#[derive(Clone, Debug)]
pub(crate) struct AiCompletionRequest {
    pub(crate) preferences: AiPreferences,
    pub(crate) instruction: String,
    pub(crate) context_markdown: String,
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Deserialize)]
struct ChatCompletionMessage {
    content: String,
}

pub(crate) fn complete_markdown(request: AiCompletionRequest) -> anyhow::Result<String> {
    if request.preferences.provider != "openai-compatible" {
        bail!(
            "unsupported AI provider '{}'; only openai-compatible is supported",
            request.preferences.provider
        );
    }
    if request.preferences.base_url.trim().is_empty() {
        bail!("AI base URL is empty");
    }
    if request.preferences.model.trim().is_empty() {
        bail!("AI model is empty");
    }

    let api_key = resolve_api_key(&request.preferences.api_key_env)?;
    let endpoint = chat_completions_endpoint(&request.preferences.base_url);
    let user_content = format!(
        "{}\n\nMarkdown context:\n\n{}",
        request.instruction, request.context_markdown
    );
    let body = ChatCompletionRequest {
        model: request.preferences.model.trim(),
        messages: vec![
            ChatMessage {
                role: "system",
                content: DEFAULT_SYSTEM_PROMPT,
            },
            ChatMessage {
                role: "user",
                content: &user_content,
            },
        ],
        temperature: 0.2,
        stream: false,
    };

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .context("AI API key contains invalid header characters")?,
    );

    let response = reqwest::blocking::Client::new()
        .post(endpoint)
        .headers(headers)
        .body(serde_json::to_vec(&body).context("failed to encode AI request")?)
        .send()
        .context("failed to send AI request")?;
    let status = response.status();
    let text = response.text().context("failed to read AI response")?;
    if !status.is_success() {
        bail!("AI request failed with {status}: {text}");
    }
    let response: ChatCompletionResponse =
        serde_json::from_str(&text).context("failed to parse AI response")?;
    let content = response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content.trim().to_string())
        .filter(|content| !content.is_empty())
        .context("AI response did not contain message content")?;
    Ok(content)
}

fn chat_completions_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

fn resolve_api_key(config_value: &str) -> anyhow::Result<String> {
    let value = config_value.trim();
    if value.is_empty() {
        bail!("AI API key is empty");
    }
    if let Ok(api_key) = std::env::var(value)
        && !api_key.trim().is_empty()
    {
        return Ok(api_key);
    }
    if looks_like_direct_api_key(value) {
        return Ok(value.to_string());
    }
    bail!(
        "missing API key environment variable '{}'. You can also paste the API key directly in preferences.",
        value
    );
}

fn looks_like_direct_api_key(value: &str) -> bool {
    value.starts_with("sk-")
        || value.starts_with("sk_")
        || value.starts_with("Bearer ")
        || value.len() >= 32 && value.chars().any(|ch| ch == '-' || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::{chat_completions_endpoint, looks_like_direct_api_key};

    #[test]
    fn builds_openai_compatible_endpoint() {
        assert_eq!(
            chat_completions_endpoint("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_endpoint("http://localhost:11434/v1/chat/completions"),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn recognizes_direct_api_key_values() {
        assert!(looks_like_direct_api_key("sk-abc12345678901234567890123456789"));
        assert!(looks_like_direct_api_key("Bearer abc12345678901234567890123456789"));
        assert!(!looks_like_direct_api_key("OPENAI_API_KEY"));
    }
}
