//! OpenAI-compatible chat completions client used by editor AI actions.

use std::io::Read;

use anyhow::{Context as _, bail};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::config::AiPreferences;

pub(crate) const DEFAULT_SYSTEM_PROMPT: &str = "You are an assistant embedded in a Markdown notes editor. Return concise Markdown only. Do not wrap the whole answer in a code block unless the user asks for code.";

#[derive(Clone, Debug)]
pub(crate) struct AiCompletionRequest {
    pub(crate) preferences: AiPreferences,
    pub(crate) instruction: String,
    pub(crate) context_markdown: String,
}

#[derive(Clone, Debug)]
pub struct AiChatTurn {
    pub role: &'static str,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct AiChatCompletionRequest {
    pub preferences: AiPreferences,
    pub system_prompt: String,
    pub turns: Vec<AiChatTurn>,
    /// Markdown context appended to the last user turn when present.
    pub context_markdown: Option<String>,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<OwnedChatMessage>,
    temperature: f32,
    stream: bool,
}

#[derive(Serialize)]
struct OwnedChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatCompletionStreamResponse {
    choices: Vec<ChatCompletionStreamChoice>,
}

#[derive(Deserialize)]
struct ChatCompletionStreamChoice {
    delta: ChatCompletionDelta,
}

#[derive(Deserialize)]
struct ChatCompletionDelta {
    content: Option<String>,
}

pub(crate) fn complete_markdown_streaming(
    request: AiCompletionRequest,
    on_delta: impl FnMut(String),
) -> anyhow::Result<String> {
    let user_content = format!(
        "{}\n\nMarkdown context:\n\n{}",
        request.instruction, request.context_markdown
    );
    complete_chat_streaming(
        AiChatCompletionRequest {
            preferences: request.preferences,
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            turns: vec![AiChatTurn {
                role: "user",
                content: user_content,
            }],
            context_markdown: None,
        },
        on_delta,
    )
}

pub(crate) fn complete_chat_streaming(
    request: AiChatCompletionRequest,
    mut on_delta: impl FnMut(String),
) -> anyhow::Result<String> {
    if request.turns.is_empty() {
        bail!("AI chat request must include at least one turn");
    }
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
    let messages = build_chat_messages(&request);
    let body = ChatCompletionRequest {
        model: request.preferences.model.trim().to_string(),
        messages,
        temperature: 0.2,
        stream: true,
    };

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .context("AI API key contains invalid header characters")?,
    );

    let mut response = reqwest::blocking::Client::new()
        .post(endpoint)
        .headers(headers)
        .body(serde_json::to_vec(&body).context("failed to encode AI request")?)
        .send()
        .context("failed to send AI request")?;
    let status = response.status();
    if !status.is_success() {
        let text = response.text().context("failed to read AI error response")?;
        bail!("AI request failed with {status}: {text}");
    }

    let mut output = String::new();
    let mut pending = String::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = response
            .read(&mut buf)
            .context("failed to read AI streaming response")?;
        if read == 0 {
            break;
        }
        pending.push_str(&String::from_utf8_lossy(&buf[..read]));
        while let Some(index) = pending.find("\n\n") {
            let event = pending[..index].to_string();
            pending.drain(..index + 2);
            if parse_stream_event(&event, &mut output, &mut on_delta)? {
                return finalize_stream_output(output);
            }
        }
    }
    if !pending.trim().is_empty() {
        parse_stream_event(&pending, &mut output, &mut on_delta)?;
    }
    finalize_stream_output(output)
}

fn build_chat_messages(request: &AiChatCompletionRequest) -> Vec<OwnedChatMessage> {
    let mut messages = vec![OwnedChatMessage {
        role: "system".to_string(),
        content: request.system_prompt.clone(),
    }];

    let last_index = request.turns.len().saturating_sub(1);
    for (index, turn) in request.turns.iter().enumerate() {
        let content = if index == last_index
            && turn.role == "user"
            && let Some(context) = request.context_markdown.as_ref()
            && !context.trim().is_empty()
        {
            format!("{}\n\nMarkdown context:\n\n{context}", turn.content)
        } else {
            turn.content.clone()
        };
        messages.push(OwnedChatMessage {
            role: turn.role.to_string(),
            content,
        });
    }

    messages
}

fn parse_stream_event(
    event: &str,
    output: &mut String,
    on_delta: &mut impl FnMut(String),
) -> anyhow::Result<bool> {
    for line in event.lines() {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data == "[DONE]" {
            return Ok(true);
        }
        if data.is_empty() {
            continue;
        }
        let chunk: ChatCompletionStreamResponse = serde_json::from_str(data)
            .with_context(|| format!("failed to parse AI stream chunk: {data}"))?;
        for choice in chunk.choices {
            if let Some(content) = choice.delta.content
                && !content.is_empty()
            {
                output.push_str(&content);
                on_delta(content);
            }
        }
    }
    Ok(false)
}

fn finalize_stream_output(output: String) -> anyhow::Result<String> {
    let content = output.trim().to_string();
    if content.is_empty() {
        bail!("AI response did not contain streamed content");
    }
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
    use super::{
        AiChatCompletionRequest, AiChatTurn, build_chat_messages, chat_completions_endpoint,
        complete_chat_streaming, looks_like_direct_api_key,
    };
    use crate::config::AiPreferences;

    fn sample_preferences() -> AiPreferences {
        AiPreferences {
            provider: "openai-compatible".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            model: "gpt-test".to_string(),
            api_key_env: "TEST_KEY".to_string(),
            allow_full_document_context: true,
            allow_workspace_context: false,
            allow_command_context: false,
            selection_toolbar: Vec::new(),
        }
    }

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

    #[test]
    fn serializes_chat_message_sequence() {
        let request = AiChatCompletionRequest {
            preferences: sample_preferences(),
            system_prompt: "system prompt".to_string(),
            turns: vec![
                AiChatTurn {
                    role: "user",
                    content: "first question".to_string(),
                },
                AiChatTurn {
                    role: "assistant",
                    content: "first answer".to_string(),
                },
                AiChatTurn {
                    role: "user",
                    content: "follow up".to_string(),
                },
            ],
            context_markdown: Some("selected text".to_string()),
        };

        let messages = build_chat_messages(&request);
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[1].content, "first question");
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages[3].role, "user");
        assert_eq!(
            messages[3].content,
            "follow up\n\nMarkdown context:\n\nselected text"
        );
    }

    #[test]
    fn rejects_empty_chat_turns() {
        let request = AiChatCompletionRequest {
            preferences: sample_preferences(),
            system_prompt: "system".to_string(),
            turns: vec![],
            context_markdown: None,
        };
        let err = complete_chat_streaming(request, |_| {}).unwrap_err();
        assert!(
            err.to_string()
                .contains("AI chat request must include at least one turn")
        );
    }
}
