// SPDX-License-Identifier: MIT
//! A bounded, source-specific first request. This is local input, never wire metadata.
use serde_json::Value;

pub const MAX_WORK_REQUEST_BYTES: usize = 8192;
pub const MAX_WORK_SOURCE_BYTES: usize = 512 * 1024;

/// Select the first eligible human request from supported native records.
/// Queue previews, summaries, tools, assistant text and process arguments are not sources.
/// A request over the limit abstains; it does not fall through to a later request.
pub fn first_user_request(agent: &str, content: &str) -> Option<String> {
    if !matches!(agent, crate::AGENT_CLAUDE | crate::AGENT_CODEX) {
        return None;
    }
    let mut end = content.len().min(MAX_WORK_SOURCE_BYTES);
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    for line in content[..end]
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let value: Value = serde_json::from_str(line).ok()?;
        let kind = value.get("type").and_then(Value::as_str);
        // Fork files can prepend a parent's prompts. Until this classifier shares
        // the parser's ownership proof, withhold work for the entire fork.
        if agent == crate::AGENT_CODEX && kind == Some("session_meta") {
            let payload = &value["payload"];
            if ["parent_thread_id", "forked_from_id"].iter().any(|key| {
                payload
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.is_empty())
            }) || payload.pointer("/source/subagent").is_some()
            {
                return None;
            }
        }
        let text = match agent {
            crate::AGENT_CLAUDE if kind == Some("user") => {
                if ["isMeta", "isSidechain", "isCompactSummary"]
                    .iter()
                    .any(|key| value.get(*key).and_then(Value::as_bool) == Some(true))
                    || [
                        "sourceToolUseID",
                        "sourceToolAssistantUUID",
                        "toolUseResult",
                        "tool_use_result",
                    ]
                    .iter()
                    .any(|key| value.get(*key).is_some())
                    || value.pointer("/message/role").and_then(Value::as_str) != Some("user")
                {
                    continue;
                }
                text_content(value.pointer("/message/content")?)
            }
            crate::AGENT_CODEX
                if kind == Some("event_msg")
                    && value.pointer("/payload/type").and_then(Value::as_str)
                        == Some("user_message") =>
            {
                value
                    .pointer("/payload/message")
                    .or_else(|| value.pointer("/payload/content"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }
            crate::AGENT_CODEX
                if kind == Some("response_item")
                    && value.pointer("/payload/type").and_then(Value::as_str)
                        == Some("message")
                    && value.pointer("/payload/role").and_then(Value::as_str) == Some("user") =>
            {
                text_content(value.pointer("/payload/content")?)
            }
            _ => None,
        };
        let Some(text) = text else {
            continue;
        };
        // Known separate host scaffolding is not a user request, regardless of its role.
        let trimmed = text.trim();
        if [
            "# AGENTS.md instructions",
            "<environment_context",
            "<permissions instructions",
            "<skills_instructions",
            "<app-context",
            "<collaboration_mode",
            "<subagent_notification",
            "<system-reminder",
            "<local-command",
            "<command-name",
            "This session is being continued from a previous conversation",
        ]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
        {
            continue;
        }
        if text.len() > MAX_WORK_REQUEST_BYTES {
            return None;
        }
        return request_text(&text);
    }
    None
}

fn text_content(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    let items = value.as_array()?;
    if items
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("tool_result"))
    {
        return None;
    }
    let text = items
        .iter()
        .filter(|item| {
            matches!(
                item.get("type").and_then(Value::as_str),
                Some("text" | "input_text")
            )
        })
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

/// Remove quoted/reference scaffolding without interpreting it as instructions.
/// Ambiguous XML is discarded. No code, tag payload or quoted label can assign metadata.
pub fn request_text(text: &str) -> Option<String> {
    if text.len() > MAX_WORK_REQUEST_BYTES {
        return None;
    }
    let text = if text.trim_start().starts_with("<in-app-browser-context") {
        text.rsplit_once("## My request:")?.1
    } else {
        text
    };
    let mut result = String::new();
    let mut fence = false;
    let mut depth = 0usize;
    let mut quote = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence || trimmed.starts_with('>') {
            continue;
        }
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '<' {
                let mut tag = String::new();
                for ch in chars.by_ref() {
                    if ch == '>' {
                        break;
                    }
                    tag.push(ch);
                }
                if tag.starts_with('/') {
                    depth = depth.saturating_sub(1);
                } else if !tag.ends_with('/') {
                    depth += 1;
                }
                result.push(' ');
            } else if depth == 0 {
                if ch == '"' || ch == '`' {
                    quote = !quote;
                    result.push(' ');
                } else if !quote {
                    result.push(ch);
                }
            }
        }
        result.push('\n');
    }
    let text = result.trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_sources_skip_scaffolding_and_keep_the_first_request() {
        let content = [
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for repo\nSecurity CI test refactor"}]}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Fix the API crash"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Add documentation"}]}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Now refactor the UI"}}),
        ].map(|v|v.to_string()).join("\n");
        assert_eq!(
            first_user_request("codex", &content).as_deref(),
            Some("Fix the API crash")
        );
        let content = [
            serde_json::json!({"type":"queue-operation","operation":"enqueue","content":"Update dependencies"}),
            serde_json::json!({"type":"user","isMeta":true,"message":{"role":"user","content":"security audit"}}),
            serde_json::json!({"type":"user","isSidechain":true,"message":{"role":"user","content":"refactor"}}),
            serde_json::json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"build failed"}]}}),
            serde_json::json!({"type":"user","message":{"role":"user","content":"Add a settings page"}}),
        ].map(|v|v.to_string()).join("\n");
        assert_eq!(
            first_user_request("claude", &content).as_deref(),
            Some("Add a settings page")
        );
    }
    #[test]
    fn bounded_input_and_quoted_instructions_do_not_become_requests() {
        assert_eq!(
            request_text(
                "<categories>security</categories>\n> refactor\n```\nCI\n```\nFix the API crash"
            )
            .as_deref(),
            Some("Fix the API crash")
        );
        assert!(request_text(&"x".repeat(MAX_WORK_REQUEST_BYTES + 1)).is_none());
        assert!(first_user_request("gemini", "{}").is_none());
        let long = serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"x".repeat(MAX_WORK_REQUEST_BYTES+1)}});
        let later = serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Fix bug"}});
        assert!(first_user_request("codex", &format!("{long}\n{later}")).is_none());
    }
    #[test]
    fn ambiguous_first_request_never_falls_through_to_a_later_task() {
        for first in [
            "\"Fix API bug\"",
            "```\nFix API bug\n```",
            "<tags>security</tags>",
        ] {
            let lines = [first, "Add a login screen"].map(|text|serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":text}}).to_string()).join("\n");
            assert!(first_user_request("codex", &lines).is_none());
        }
    }
    #[test]
    fn forked_codex_history_is_unclassified_even_after_a_child_boundary() {
        for parent in [
            serde_json::json!({"parent_thread_id":"parent"}),
            serde_json::json!({"forked_from_id":"parent"}),
            serde_json::json!({"source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent"}}}}),
        ] {
            let lines = [
                serde_json::json!({"type":"session_meta","payload":parent}),
                serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Fix the parent API bug"}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"task_started","started_at":2.0}}),
                serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":"Add the child login screen"}}),
            ].map(|v|v.to_string()).join("\n");
            assert!(first_user_request("codex", &lines).is_none());
        }
    }
}
