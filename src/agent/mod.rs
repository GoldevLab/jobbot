//! ADK OpenRouter LLM client for scoring + drafting.

use adk_core::{
    Content, GenerateContentConfig, Llm, LlmRequest, LlmResponseStream, Part,
};
use adk_model::openrouter::{
    OPENROUTER_API_BASE, OpenRouterApiMode, OpenRouterClient, OpenRouterConfig,
};
use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct LlmAgent {
    client: Arc<OpenRouterClient>,
    model: String,
}

impl LlmAgent {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENROUTER_API_KEY")
            .context("OPENROUTER_API_KEY missing — set it in .env")?;
        let model = std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "openai/gpt-oss-20b:free".into());
        let base = std::env::var("OPENROUTER_BASE_URL")
            .unwrap_or_else(|_| OPENROUTER_API_BASE.to_string());
        let site = std::env::var("OPENROUTER_SITE_URL")
            .unwrap_or_else(|_| "https://github.com/GoldevLab/jobbot".into());
        let title =
            std::env::var("OPENROUTER_APP_NAME").unwrap_or_else(|_| "JobBot".into());

        let client = OpenRouterClient::new(
            OpenRouterConfig::new(api_key, &model)
                .with_base_url(base)
                .with_http_referer(site)
                .with_title(title)
                .with_default_api_mode(OpenRouterApiMode::ChatCompletions),
        )
        .map_err(|e| anyhow!("OpenRouter client: {e}"))?;

        Ok(Self {
            client: Arc::new(client),
            model,
        })
    }

    pub async fn complete(&self, prompt: &str) -> Result<String> {
        self.complete_with_tokens(prompt, 900).await
    }

    pub async fn complete_with_tokens(&self, prompt: &str, max_tokens: i32) -> Result<String> {
        let config = GenerateContentConfig {
            max_output_tokens: Some(max_tokens),
            temperature: Some(0.45),
            ..Default::default()
        };
        let req = LlmRequest::new(
            self.model.clone(),
            vec![Content::new("user").with_text(prompt)],
        )
        .with_config(config);

        let stream = self
            .client
            .generate_content(req, false)
            .await
            .map_err(|e| anyhow!("generate_content: {e}"))?;

        collect_text(stream).await
    }

    pub async fn complete_json(&self, prompt: &str) -> Result<Value> {
        let raw = self.complete_with_tokens(prompt, 2200).await?;
        if let Ok(v) = parse_json_loose(&raw) {
            return Ok(v);
        }
        // Free models often truncate mid-JSON — ask once to finish.
        let repair_prompt = format!(
            "The JSON below is incomplete. Output ONLY the completed valid JSON object. No markdown, no commentary.\n\n{}",
            &raw[..raw.len().min(3500)]
        );
        let raw2 = self.complete_with_tokens(&repair_prompt, 2200).await?;
        parse_json_loose(&raw2).or_else(|_| parse_json_loose(&raw))
    }
}

async fn collect_text(mut stream: LlmResponseStream) -> Result<String> {
    let mut out = String::new();
    let mut reasoning = String::new();
    while let Some(item) = stream.next().await {
        let resp = item.map_err(|e| anyhow!("llm stream: {e}"))?;
        if let Some(content) = resp.content {
            for part in content.parts {
                match part {
                    Part::Text { text } => out.push_str(&text),
                    Part::Thinking { thinking, .. } => reasoning.push_str(&thinking),
                    _ => {}
                }
            }
        }
    }
    if out.trim().is_empty() && !reasoning.trim().is_empty() {
        out = reasoning;
    }
    if out.trim().is_empty() {
        return Err(anyhow!("empty LLM response"));
    }
    Ok(out)
}

fn parse_json_loose(raw: &str) -> Result<Value> {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Ok(v);
    }
    if let Some(start) = trimmed.find('{') {
        let slice = &trimmed[start..];
        if let Some(end) = slice.rfind('}') {
            let candidate = &slice[..=end];
            if let Ok(v) = serde_json::from_str::<Value>(candidate) {
                return Ok(v);
            }
        }
        if let Ok(v) = repair_truncated_json(slice) {
            return Ok(v);
        }
    }
    Err(anyhow!(
        "could not parse JSON from LLM: {}",
        &trimmed[..trimmed.len().min(240)]
    ))
}

/// Close open strings / braces when the free model hits the token limit mid-object.
fn repair_truncated_json(s: &str) -> Result<Value> {
    let mut t = s.trim().to_string();
    if t.is_empty() {
        return Err(anyhow!("empty"));
    }

    // Drop a trailing incomplete key like `,"foo` without value
    if let Some(idx) = t.rfind(",\"") {
        let tail = &t[idx + 2..];
        if !tail.contains(':') && !tail.contains('"') {
            t.truncate(idx);
        }
    }

    let mut in_str = false;
    let mut escape = false;
    for c in t.chars() {
        if escape {
            escape = false;
            continue;
        }
        if in_str && c == '\\' {
            escape = true;
            continue;
        }
        if c == '"' {
            in_str = !in_str;
        }
    }
    if in_str {
        t.push('"');
    }

    let mut stack: Vec<char> = Vec::new();
    in_str = false;
    escape = false;
    for c in t.chars() {
        if escape {
            escape = false;
            continue;
        }
        if in_str && c == '\\' {
            escape = true;
            continue;
        }
        if c == '"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        match c {
            '{' | '[' => stack.push(c),
            '}' => {
                let _ = stack.pop();
            }
            ']' => {
                let _ = stack.pop();
            }
            _ => {}
        }
    }
    // Remove trailing comma before we close
    let trimmed_end = t.trim_end();
    if trimmed_end.ends_with(',') {
        t = trimmed_end.trim_end_matches(',').to_string();
    }
    while let Some(open) = stack.pop() {
        t.push(if open == '{' { '}' } else { ']' });
    }

    serde_json::from_str(&t).map_err(|e| anyhow!("repair failed: {e}"))
}
