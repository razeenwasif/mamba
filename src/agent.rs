//! The AI tutor: a streaming chat client for a local [Ollama] server.
//!
//! A request runs on a worker thread and streams tokens back over an [`mpsc`]
//! channel — the same shape as the verification worker — so the immediate-mode
//! UI keeps redrawing (and stays responsive to input) while the model
//! generates. The app drains the channel each tick via `App::poll_agent`.
//!
//! Configuration (both optional):
//! * `MAMBA_OLLAMA_URL`   — server base URL (default `http://localhost:11434`)
//! * `MAMBA_OLLAMA_MODEL` — model tag (default `gemma4:12b-it-qat`)
//!
//! [Ollama]: https://ollama.com

use std::io::{BufRead, BufReader};
use std::sync::mpsc::{self, Receiver};
use std::thread;

/// One turn in the conversation. `role` is `"system"`, `"user"`, or
/// `"assistant"` — the roles Ollama's chat API expects.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// An event streamed back from a running chat request.
#[derive(Debug)]
pub enum AgentEvent {
    /// An incremental chunk of the assistant's reply.
    Token(String),
    /// The reply finished normally.
    Done,
    /// The request failed (server down, bad model, network error, …).
    Error(String),
}

/// The model tag to use, from `MAMBA_OLLAMA_MODEL` or the default.
pub fn default_model() -> String {
    std::env::var("MAMBA_OLLAMA_MODEL").unwrap_or_else(|_| "gemma4:12b-it-qat".to_string())
}

/// The server base URL, from `MAMBA_OLLAMA_URL` or the default.
pub fn default_url() -> String {
    std::env::var("MAMBA_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string())
}

/// Starts a streaming chat completion on a background thread and returns the
/// receiving end of its token stream. The thread owns the HTTP request; the
/// caller polls the returned channel without blocking.
pub fn stream_chat(url: String, model: String, messages: Vec<ChatMessage>) -> Receiver<AgentEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let body = serde_json::json!({
            "model": model,
            "messages": messages
                .iter()
                .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
                .collect::<Vec<_>>(),
            "stream": true,
        });
        let endpoint = format!("{}/api/chat", url.trim_end_matches('/'));

        let response = match ureq::post(&endpoint).send_json(body) {
            Ok(resp) => resp,
            Err(err) => {
                let _ = tx.send(AgentEvent::Error(format!(
                    "Ollama request failed: {err}. Is `ollama serve` running and is the model pulled?"
                )));
                return;
            }
        };

        // Ollama streams newline-delimited JSON: one object per chunk, each
        // with `message.content` and a `done` flag.
        let reader = BufReader::new(response.into_reader());
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(err) => {
                    let _ = tx.send(AgentEvent::Error(format!("stream read error: {err}")));
                    return;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if let Some(chunk) = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                && !chunk.is_empty()
            {
                let _ = tx.send(AgentEvent::Token(chunk.to_string()));
            }
            if value.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                let _ = tx.send(AgentEvent::Done);
                return;
            }
        }
        let _ = tx.send(AgentEvent::Done);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits a *live* Ollama server, so it's ignored by default. Run with:
    /// `cargo test -- --ignored streams_a_reply` (optionally
    /// `MAMBA_OLLAMA_MODEL=gemma3:latest` for a faster warm model).
    #[test]
    #[ignore = "requires a running ollama server with the model pulled"]
    fn streams_a_reply_from_ollama() {
        let rx = stream_chat(
            default_url(),
            default_model(),
            vec![ChatMessage::user("Reply with exactly: OK")],
        );
        let (mut reply, mut done) = (String::new(), false);
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(120)) {
                Ok(AgentEvent::Token(t)) => reply.push_str(&t),
                Ok(AgentEvent::Done) => {
                    done = true;
                    break;
                }
                Ok(AgentEvent::Error(e)) => panic!("agent error: {e}"),
                Err(_) => break,
            }
        }
        assert!(done, "stream never finished");
        assert!(!reply.trim().is_empty(), "no tokens received");
        eprintln!("model replied: {:?}", reply.trim());
    }
}
