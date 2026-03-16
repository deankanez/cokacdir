use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use serde_json::Value;

use crate::services::claude::{debug_log_to, StreamMessage, CancelToken, kill_child_tree};

/// Cached path to the codex binary.
static CODEX_PATH: OnceLock<Option<String>> = OnceLock::new();

/// Resolve the path to the codex binary.
/// First tries `which codex`, then falls back to `bash -lc "which codex"`.
#[cfg(unix)]
fn resolve_codex_path() -> Option<String> {
    if let Ok(output) = Command::new("which").arg("codex").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }

    if let Ok(output) = Command::new("bash")
        .args(["-lc", "which codex"])
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }

    None
}

#[cfg(windows)]
fn resolve_codex_path() -> Option<String> {
    if let Ok(output) = Command::new("where").arg("codex").output() {
        if output.status.success() {
            let all = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // On Windows, npm installs both `codex` (Unix shell script) and
            // `codex.cmd` (Windows batch wrapper).  `where codex` may return both.
            // We must prefer the .cmd variant; the bare file is not executable.
            let lines: Vec<&str> = all.lines().collect();

            // 1) Prefer a .cmd line directly from `where` output
            if let Some(cmd) = lines.iter().find(|l| l.ends_with(".cmd")) {
                return Some(cmd.to_string());
            }

            // 2) If only the bare file was returned, check if a .cmd sibling exists
            if let Some(first) = lines.first() {
                if !first.is_empty() {
                    let cmd_sibling = format!("{}.cmd", first);
                    if std::path::Path::new(&cmd_sibling).exists() {
                        return Some(cmd_sibling);
                    }
                    return Some(first.to_string());
                }
            }
        }
    }
    None
}

/// Get the cached codex binary path, resolving it on first call.
fn get_codex_path() -> Option<&'static str> {
    CODEX_PATH.get_or_init(|| resolve_codex_path()).as_deref()
}

/// Check if Codex CLI is available
pub fn is_codex_available() -> bool {
    get_codex_path().is_some()
}

/// Check if a model string refers to the Codex backend
pub fn is_codex_model(model: Option<&str>) -> bool {
    model.map(|m| m == "codex" || m.starts_with("codex:")).unwrap_or(false)
}

/// Strip "codex:" prefix and return the actual model name.
/// Returns None if the input is just "codex" (use CLI default).
pub fn strip_codex_prefix(model: &str) -> Option<&str> {
    model.strip_prefix("codex:").filter(|s| !s.is_empty())
}

fn codex_debug_log(msg: &str) {
    debug_log_to("codex.log", msg);
}

/// Execute a command using Codex CLI with streaming output.
///
/// Parameters mirror `claude::execute_command_streaming` for consistency,
/// but some are ignored (allowed_tools, no_session_persistence)
/// because Codex exec has no tool restriction support.
///
/// When `session_id` is Some, uses `codex exec resume` to continue an existing
/// session (Codex manages conversation history natively). When None, starts a
/// new session with `codex exec`.
pub fn execute_command_streaming(
    prompt: &str,
    session_id: Option<&str>,
    working_dir: &str,
    sender: Sender<StreamMessage>,
    system_prompt: Option<&str>,
    _allowed_tools: Option<&[String]>, // ignored — Codex has no tool restriction
    cancel_token: Option<std::sync::Arc<CancelToken>>,
    model: Option<&str>,               // "codex:" prefix already stripped
    _no_session_persistence: bool,     // ignored — Codex exec handles persistence internally
) -> Result<(), String> {
    codex_debug_log("========================================");
    codex_debug_log("=== codex execute_command_streaming START ===");
    codex_debug_log("========================================");
    codex_debug_log(&format!("prompt_len: {} chars", prompt.len()));
    codex_debug_log(&format!("session_id: {:?}", session_id));
    codex_debug_log(&format!("working_dir: {}", working_dir));
    codex_debug_log(&format!("model: {:?}", model));

    let resuming = session_id.is_some();

    // Build effective prompt: prepend system prompt only for new sessions
    // (resumed sessions already have the system prompt from the initial turn)
    let effective_prompt = if resuming {
        prompt.to_string()
    } else {
        match system_prompt {
            Some(sp) if !sp.is_empty() => format!("{}\n\n---\n\n{}", sp, prompt),
            _ => prompt.to_string(),
        }
    };

    // Build CLI arguments
    let mut args = if let Some(sid) = session_id {
        // codex exec resume --json --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check <session_id> -
        vec![
            "exec".to_string(),
            "resume".to_string(),
            "--json".to_string(),
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
            "--skip-git-repo-check".to_string(),
            sid.to_string(),
        ]
    } else {
        // codex exec --json --dangerously-bypass-approvals-and-sandbox --skip-git-repo-check -C <dir> -
        vec![
            "exec".to_string(),
            "--json".to_string(),
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
            "--skip-git-repo-check".to_string(),
            "-C".to_string(),
            working_dir.to_string(),
        ]
    };

    if let Some(m) = model {
        args.push("-m".to_string());
        args.push(m.to_string());
    }

    // `-` means read prompt from stdin
    args.push("-".to_string());

    let codex_bin = get_codex_path()
        .ok_or_else(|| {
            codex_debug_log("ERROR: Codex CLI not found");
            "Codex CLI not found. Is Codex CLI installed?".to_string()
        })?;

    codex_debug_log("--- Spawning codex process ---");
    codex_debug_log(&format!("Command: {} {:?}", codex_bin, args));

    let spawn_start = std::time::Instant::now();
    let mut child = Command::new(codex_bin)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            codex_debug_log(&format!("ERROR: Failed to spawn: {}", e));
            format!("Failed to start Codex: {}. Is Codex CLI installed?", e)
        })?;
    codex_debug_log(&format!("Codex process spawned in {:?}, pid={:?}", spawn_start.elapsed(), child.id()));

    // Store child PID in cancel token
    if let Some(ref token) = cancel_token {
        *token.child_pid.lock().unwrap() = Some(child.id());
        // If /stop arrived before PID was stored, kill immediately
        if token.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            kill_child_tree(&mut child);
            let _ = child.wait();
            return Ok(());
        }
    }

    // Write prompt to stdin
    if let Some(mut stdin) = child.stdin.take() {
        codex_debug_log(&format!("Writing prompt to stdin ({} bytes)...", effective_prompt.len()));
        let _ = stdin.write_all(effective_prompt.as_bytes());
        codex_debug_log("stdin dropped (closed)");
    }

    // Drain stderr in a background thread to prevent deadlock
    // (if child writes >64KB to stderr while we block reading stdout, both sides block)
    let stderr_thread = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            std::io::read_to_string(stderr).unwrap_or_default()
        })
    });

    // Read stdout line by line (JSONL)
    let stdout = child.stdout.take()
        .ok_or_else(|| {
            codex_debug_log("ERROR: Failed to capture stdout");
            "Failed to capture stdout".to_string()
        })?;
    let reader = BufReader::new(stdout);

    let mut last_session_id: Option<String> = None;
    let mut got_done = false;
    let mut stdout_error: Option<(String, String)> = None;
    let mut line_count = 0;

    codex_debug_log("Entering JSONL lines loop...");
    'lines: for line in reader.lines() {
        // Check cancel token
        if let Some(ref token) = cancel_token {
            if token.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                codex_debug_log("Cancel detected — killing child process");
                kill_child_tree(&mut child);
                let _ = child.wait();
                return Ok(());
            }
        }

        let line = match line {
            Ok(l) => l,
            Err(e) => {
                codex_debug_log(&format!("ERROR: Failed to read line: {}", e));
                let _ = sender.send(StreamMessage::Error {
                    message: format!("Failed to read output: {}", e),
                    stdout: String::new(), stderr: String::new(), exit_code: None,
                });
                break;
            }
        };

        line_count += 1;

        if line.trim().is_empty() {
            continue;
        }

        let line_preview: String = line.chars().take(200).collect();
        codex_debug_log(&format!("Line {}: {}", line_count, line_preview));

        if let Ok(json) = serde_json::from_str::<Value>(&line) {
            let messages = parse_codex_event(&json);
            codex_debug_log(&format!("  Parsed {} messages", messages.len()));

            for msg in messages {
                match &msg {
                    StreamMessage::Init { session_id } => {
                        codex_debug_log(&format!("  >>> Init: session_id={}", session_id));
                        last_session_id = Some(session_id.clone());
                    }
                    StreamMessage::Done { .. } => {
                        codex_debug_log("  >>> Done");
                        got_done = true;
                    }
                    StreamMessage::Error { ref message, .. } => {
                        codex_debug_log(&format!("  >>> Error: {}", message));
                        stdout_error = Some((message.clone(), line.clone()));
                        continue;
                    }
                    StreamMessage::Text { content } => {
                        let preview: String = content.chars().take(100).collect();
                        codex_debug_log(&format!("  >>> Text: {} chars, preview: {:?}", content.len(), preview));
                    }
                    StreamMessage::ToolUse { name, input } => {
                        let input_preview: String = input.chars().take(200).collect();
                        codex_debug_log(&format!("  >>> ToolUse: name={}, input={:?}", name, input_preview));
                    }
                    StreamMessage::ToolResult { content, is_error } => {
                        codex_debug_log(&format!("  >>> ToolResult: is_error={}, len={}", is_error, content.len()));
                    }
                    StreamMessage::TaskNotification { .. } => {}
                }

                if sender.send(msg).is_err() {
                    codex_debug_log("  ERROR: Channel send failed (receiver dropped)");
                    break 'lines;
                }
            }
        } else {
            codex_debug_log(&format!("  NOT valid JSON: {}", line_preview));
        }
    }

    codex_debug_log(&format!("--- Exited lines loop, {} lines read ---", line_count));

    // Check cancel after loop
    if let Some(ref token) = cancel_token {
        if token.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            codex_debug_log("Cancel detected after loop — killing child process");
            kill_child_tree(&mut child);
            let _ = child.wait();
            return Ok(());
        }
    }

    // Wait for process to finish
    let status = child.wait().map_err(|e| {
        codex_debug_log(&format!("ERROR: Process wait failed: {}", e));
        format!("Process error: {}", e)
    })?;
    codex_debug_log(&format!("Process finished, exit_code: {:?}", status.code()));

    // Handle errors
    if stdout_error.is_some() || !status.success() {
        let stderr_msg = stderr_thread
            .and_then(|h| h.join().ok())
            .unwrap_or_default();

        let (message, stdout_raw) = if let Some((msg, raw)) = stdout_error {
            (msg, raw)
        } else {
            (format!("Process exited with code {:?}", status.code()), String::new())
        };

        codex_debug_log(&format!("Sending error: message={}", message));
        let _ = sender.send(StreamMessage::Error {
            message,
            stdout: stdout_raw,
            stderr: stderr_msg,
            exit_code: status.code(),
        });
        return Ok(());
    }

    // Send synthetic Done if not received
    if !got_done {
        codex_debug_log("No Done message received, sending synthetic Done");
        let _ = sender.send(StreamMessage::Done {
            result: String::new(),
            session_id: last_session_id,
        });
    }

    codex_debug_log("=== codex execute_command_streaming END (success) ===");
    Ok(())
}

/// Parse a Codex JSONL event into zero or more StreamMessages.
///
/// Returns Vec because some events (e.g. command_execution) produce
/// both ToolUse and ToolResult messages at once.
fn parse_codex_event(json: &Value) -> Vec<StreamMessage> {
    let event_type = match json.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return vec![],
    };

    match event_type {
        // Thread started — contains thread_id
        "thread.started" => {
            let thread_id = json.get("thread_id")
                .or_else(|| json.get("thread").and_then(|t| t.get("id")))
                .and_then(|v| v.as_str())
                .unwrap_or("codex-session")
                .to_string();
            vec![StreamMessage::Init { session_id: thread_id }]
        }

        // Item completed — the main content carrier
        "item.completed" => {
            parse_item_completed(json)
        }

        // Turn completed — marks end of response
        "turn.completed" => {
            vec![StreamMessage::Done {
                result: String::new(),
                session_id: None,
            }]
        }

        // turn.failed has {error: {message: "..."}}
        "turn.failed" => {
            let message = json.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown Codex error")
                .to_string();
            vec![StreamMessage::Error {
                message,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
            }]
        }

        // Top-level error event has {message: "..."}
        "error" => {
            let message = json.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown Codex error")
                .to_string();
            vec![StreamMessage::Error {
                message,
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
            }]
        }

        // Ignored events — avoid duplicates (completed handles the final state)
        "turn.started" | "item.started" | "item.updated" => vec![],

        // Unknown event types — ignore
        _ => {
            codex_debug_log(&format!("Unknown codex event type: {}", event_type));
            vec![]
        }
    }
}

/// Parse an `item.completed` event into StreamMessages.
fn parse_item_completed(json: &Value) -> Vec<StreamMessage> {
    // The item can be at top level or nested under "item"
    let item = json.get("item").unwrap_or(json);
    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match item_type {
        // Agent text message
        "agent_message" | "message" => {
            let text = extract_text_content(item);
            if text.is_empty() {
                vec![]
            } else {
                vec![StreamMessage::Text { content: text }]
            }
        }

        // Command execution — produces ToolUse + ToolResult
        // Codex fields: command, aggregated_output, exit_code, status
        "command_execution" => {
            let command = item.get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let aggregated_output = item.get("aggregated_output")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let exit_code = item.get("exit_code")
                .and_then(|v| v.as_i64());
            let is_error = exit_code.map(|c| c != 0).unwrap_or(false);

            vec![
                StreamMessage::ToolUse {
                    name: "Bash".to_string(),
                    input: serde_json::json!({"command": command, "exit_code": exit_code}).to_string(),
                },
                StreamMessage::ToolResult { content: aggregated_output, is_error },
            ]
        }

        // File change — Codex fields: changes (array of {path, kind, ...}), status
        "file_change" => {
            vec![StreamMessage::ToolUse {
                name: "FileChange".to_string(),
                input: item.to_string(),
            }]
        }

        // MCP tool call — Codex fields: server, tool, arguments, result{content,structured_content}, error{message}, status
        "mcp_tool_call" => {
            let server = item.get("server").and_then(|v| v.as_str()).unwrap_or("");
            let tool = item.get("tool").and_then(|v| v.as_str()).unwrap_or("unknown");
            let display_name = if server.is_empty() {
                tool.to_string()
            } else {
                format!("{}:{}", server, tool)
            };

            let arguments = item.get("arguments")
                .map(|v| v.to_string())
                .unwrap_or_default();

            let mut msgs = vec![
                StreamMessage::ToolUse {
                    name: display_name,
                    input: arguments,
                },
            ];

            // Check for error first (skip null — serde serializes None as null)
            if let Some(err) = item.get("error").filter(|v| !v.is_null()) {
                let message = err.get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("MCP tool call failed")
                    .to_string();
                msgs.push(StreamMessage::ToolResult { content: message, is_error: true });
            } else if let Some(result) = item.get("result").filter(|v| !v.is_null()) {
                // result has {content: [...], structured_content}
                let content = if let Some(arr) = result.get("content").and_then(|v| v.as_array()) {
                    arr.iter().filter_map(|c| {
                        c.get("text").and_then(|v| v.as_str()).map(|s| s.to_string())
                    }).collect::<Vec<_>>().join("\n")
                } else {
                    result.to_string()
                };
                msgs.push(StreamMessage::ToolResult { content, is_error: false });
            }

            msgs
        }

        // Collab tool call — sub-agent interactions (SpawnAgent, SendInput, Wait, CloseAgent)
        // Codex fields: tool, sender_thread_id, receiver_thread_ids, prompt, agents_states, status
        "collab_tool_call" => {
            let tool = item.get("tool").and_then(|v| v.as_str()).unwrap_or("unknown");
            vec![StreamMessage::ToolUse {
                name: format!("Collab:{}", tool),
                input: item.to_string(),
            }]
        }

        // Web search — Codex fields: id, query, action
        "web_search" => {
            vec![StreamMessage::ToolUse {
                name: "WebSearch".to_string(),
                input: item.to_string(),
            }]
        }

        // Todo list — agent's running plan. Codex fields: items (Vec<{text, completed}>)
        "todo_list" => {
            if let Some(items) = item.get("items").and_then(|v| v.as_array()) {
                let summary: Vec<String> = items.iter().map(|t| {
                    let text = t.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    let done = t.get("completed").and_then(|v| v.as_bool()).unwrap_or(false);
                    format!("[{}] {}", if done { "x" } else { " " }, text)
                }).collect();
                vec![StreamMessage::Text { content: summary.join("\n") }]
            } else {
                vec![]
            }
        }

        // Reasoning/thinking — internal, not shown to user
        "reasoning" => {
            codex_debug_log(&format!("reasoning (filtered): {:?}",
                extract_text_content(item).chars().take(80).collect::<String>()));
            vec![]
        }

        // Non-fatal error surfaced as an item — ErrorItem { message }
        "error" => {
            let message = item.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            vec![StreamMessage::Text { content: format!("⚠ {}", message) }]
        }

        _ => {
            // Try to extract text from unknown item types (e.g. reasoning)
            let text = extract_text_content(item);
            if text.is_empty() {
                codex_debug_log(&format!("Unknown item type: {}", item_type));
                vec![]
            } else {
                vec![StreamMessage::Text { content: text }]
            }
        }
    }
}

/// Extract text content from a Codex item.
/// Handles both direct "content" string and array-of-objects format.
fn extract_text_content(item: &Value) -> String {
    // Try direct "content" string
    if let Some(text) = item.get("content").and_then(|v| v.as_str()) {
        return text.to_string();
    }

    // Try "text" field
    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }

    // Try "content" as array of objects (OpenAI format)
    if let Some(content_arr) = item.get("content").and_then(|v| v.as_array()) {
        let mut text = String::new();
        for part in content_arr {
            if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                text.push_str(t);
            }
        }
        if !text.is_empty() {
            return text;
        }
    }

    // Try nested message.content
    if let Some(message) = item.get("message") {
        if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
            return text.to_string();
        }
        if let Some(content_arr) = message.get("content").and_then(|v| v.as_array()) {
            let mut text = String::new();
            for part in content_arr {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    text.push_str(t);
                }
            }
            if !text.is_empty() {
                return text;
            }
        }
    }

    String::new()
}

