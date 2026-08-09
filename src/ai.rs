use crate::{
    Res,
    config::{AiBackend, AiConfig},
    error::Error,
};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

/// Generate a commit message for `diff`, using whichever backend `config`
/// selects (an OpenAI-compatible HTTP API, or a local command such as the
/// `claude` / `codex` CLI).
///
/// The call is synchronous and blocks until it completes. On any failure an
/// [`Error::Ai`] is returned so callers can fall back to a plain commit. `cwd`
/// is the directory a command backend runs in (the repository working tree).
pub(crate) fn generate_commit_message(
    config: &AiConfig,
    system_prompt: &str,
    diff: &str,
    cwd: &Path,
) -> Res<String> {
    let diff = truncate_diff(diff, config.max_diff_bytes);

    match config.backend {
        AiBackend::Api => generate_via_api(config, system_prompt, &diff),
        AiBackend::Command => generate_via_command(&config.command, system_prompt, &diff, cwd),
    }
}

/// Run the configured command, writing the diff to its stdin and returning its
/// stdout as the commit message. `{prompt}` in any argument is replaced with the
/// system prompt.
fn generate_via_command(
    command: &[String],
    system_prompt: &str,
    diff: &str,
    cwd: &Path,
) -> Res<String> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| Error::Ai("[ai].command is empty".into()))?;

    let mut cmd = Command::new(program);
    for arg in args {
        cmd.arg(arg.replace("{prompt}", system_prompt));
    }
    cmd.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Ai(format!("failed to run {program}: {e}")))?;

    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(diff.as_bytes())
        .map_err(|e| Error::Ai(format!("failed to write diff to {program}: {e}")))?;

    let output = child
        .wait_with_output()
        .map_err(|e| Error::Ai(format!("failed to wait for {program}: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Ai(format!(
            "{program} exited with {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    let message = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if message.is_empty() {
        return Err(Error::Ai(format!("{program} produced no output")));
    }

    Ok(message)
}

/// Call an OpenAI-compatible chat-completions endpoint and return the message.
fn generate_via_api(config: &AiConfig, system_prompt: &str, diff: &str) -> Res<String> {
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
    use super::{generate_via_command, truncate_diff};
    use std::path::Path;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn command_pipes_diff_to_stdin() {
        // `cat` echoes stdin (the diff) back to stdout.
        let out = generate_via_command(&s(&["cat"]), "unused prompt", "the diff", Path::new("."));
        assert_eq!(out.unwrap(), "the diff");
    }

    #[test]
    fn command_substitutes_prompt_placeholder() {
        // `sh -c 'printf %s "$1"' sh <arg>` prints the argument, where {prompt}
        // has been replaced with the system prompt.
        let out = generate_via_command(
            &s(&["sh", "-c", "printf %s \"$1\"", "sh", "{prompt}"]),
            "SYSTEM PROMPT",
            "ignored diff",
            Path::new("."),
        );
        assert_eq!(out.unwrap(), "SYSTEM PROMPT");
    }

    #[test]
    fn command_empty_is_an_error() {
        assert!(generate_via_command(&[], "p", "d", Path::new(".")).is_err());
    }

    #[test]
    fn command_nonzero_exit_is_an_error() {
        assert!(generate_via_command(&s(&["false"]), "p", "d", Path::new(".")).is_err());
    }

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
