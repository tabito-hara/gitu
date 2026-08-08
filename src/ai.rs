use crate::{Res, config::AiConfig, error::Error};

/// Generate a commit message for `diff` using an OpenAI-compatible
/// chat-completions endpoint described by `config`.
///
/// The call is synchronous and blocks until the API responds. On any failure
/// (missing key, network error, non-2xx response, unexpected body) an
/// [`Error::Ai`] is returned so callers can fall back to a plain commit.
pub(crate) fn generate_commit_message(
    config: &AiConfig,
    system_prompt: &str,
    diff: &str,
) -> Res<String> {
    let diff = truncate_diff(diff, config.max_diff_bytes);

    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": config.model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": diff },
        ],
    });

    let mut request = ureq::post(&url).set("Content-Type", "application/json");

    if !config.api_key_env.is_empty() {
        let key = std::env::var(&config.api_key_env).map_err(|_| {
            Error::Ai(format!(
                "environment variable {} is not set",
                config.api_key_env
            ))
        })?;
        request = request.set("Authorization", &format!("Bearer {key}"));
    }

    let response = request.send_json(body).map_err(|e| match e {
        // Non-2xx responses: surface the status and any body the server returned.
        ureq::Error::Status(code, resp) => {
            let detail = resp
                .into_string()
                .unwrap_or_else(|_| "<unreadable body>".into());
            Error::Ai(format!("API returned status {code}: {}", detail.trim()))
        }
        ureq::Error::Transport(t) => Error::Ai(format!("request failed: {t}")),
    })?;

    let value: serde_json::Value = response
        .into_json()
        .map_err(|e| Error::Ai(format!("could not parse response as JSON: {e}")))?;

    let message = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| Error::Ai("response did not contain a message".into()))?
        .trim()
        .to_string();

    if message.is_empty() {
        return Err(Error::Ai("model returned an empty message".into()));
    }

    Ok(message)
}

/// Truncate `diff` to at most `max_bytes` bytes on a char boundary, appending a
/// marker when content was dropped. `max_bytes == 0` disables truncation.
fn truncate_diff(diff: &str, max_bytes: usize) -> String {
    if max_bytes == 0 || diff.len() <= max_bytes {
        return diff.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}\n\n[diff truncated]", &diff[..end])
}

#[cfg(test)]
mod tests {
    use super::truncate_diff;

    #[test]
    fn no_truncation_when_within_limit() {
        assert_eq!(truncate_diff("hello", 16000), "hello");
    }

    #[test]
    fn zero_disables_truncation() {
        assert_eq!(truncate_diff("hello", 0), "hello");
    }

    #[test]
    fn truncates_and_marks() {
        let out = truncate_diff("abcdefgh", 4);
        assert!(out.starts_with("abcd"));
        assert!(out.contains("[diff truncated]"));
    }

    #[test]
    fn respects_char_boundary() {
        // "é" is two bytes; cutting at 3 must not split it.
        let out = truncate_diff("aééé", 3);
        assert!(out.starts_with("aé"));
        assert!(out.contains("[diff truncated]"));
    }
}
