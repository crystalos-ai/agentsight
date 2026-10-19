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
            if trimmed.starts_with('<') && strip_scaffolding(&text).is_none() {
                return None;
            }
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
/// Any unfinished or mismatched delimiter withholds the entire request. In
/// particular, a quote is never reset at a newline to expose quoted instructions.
pub fn request_text(text: &str) -> Option<String> {
    if text.len() > MAX_WORK_REQUEST_BYTES {
        return None;
    }
    let stripped = strip_scaffolding(text)?;
    let text = if text.trim_start().starts_with("<in-app-browser-context") {
        stripped.rsplit_once("## My request:")?.1.trim()
    } else {
        stripped.trim()
    };
    (!text.is_empty()).then(|| text.to_string())
}

/// Some(empty) means balanced scaffolding without request text. None means
/// malformed input, and must not allow a later request to become the first.
fn strip_scaffolding(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut result = String::new();
    let mut tags = Vec::new();
    let mut quote = false;
    let mut code: Option<usize> = None;
    let mut fence: Option<(u8, usize)> = None;
    let mut at = 0;
    while at < bytes.len() {
        if (at == 0 || bytes[at - 1] == b'\n') && !quote && code.is_none() {
            let end = text[at..].find('\n').map_or(bytes.len(), |n| at + n);
            let line = text[at..end].trim_start();
            let marker = line.as_bytes().first().copied();
            let width = line.bytes().take_while(|ch| Some(*ch) == marker).count();
            if let Some((character, minimum)) = fence {
                if marker == Some(character) && width >= minimum && line[width..].trim().is_empty()
                {
                    fence = None;
                }
                at = (end + 1).min(bytes.len());
                result.push('\n');
                continue;
            }
            if matches!(marker, Some(b'`' | b'~')) && width >= 3 {
                fence = Some((marker?, width));
                at = (end + 1).min(bytes.len());
                result.push('\n');
                continue;
            }
            if line.starts_with('>') {
                at = (end + 1).min(bytes.len());
                result.push('\n');
                continue;
            }
        }
        let ch = text[at..].chars().next()?;
        if quote {
            if ch == '\\' {
                at += ch.len_utf8();
                // An escaped quote cannot end the reference. A dangling escape
                // leaves the quote open and is rejected below.
                if let Some(escaped) = text[at..].chars().next() {
                    at += escaped.len_utf8();
                }
                continue;
            }
            if ch == '"' {
                quote = false;
            }
        } else if let Some(width) = code {
            if ch == '`' {
                let run = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
                if run == width {
                    code = None;
                }
                at += run;
                continue;
            }
        } else if ch == '<' {
            if let Some((tag, width)) = markup(&text[at..]).ok()? {
                match tag {
                    Markup::Open(name) => tags.push(name),
                    Markup::Close(name) => {
                        if tags.pop() != Some(name) {
                            return None;
                        }
                    }
                    Markup::Standalone => {}
                }
                at += width;
                result.push(' ');
                continue;
            }
            // Comparisons such as x < y are ordinary request text.
            if tags.is_empty() {
                result.push(ch);
            }
        } else if tags.is_empty() {
            match ch {
                '"' => {
                    quote = true;
                    result.push(' ');
                }
                '`' => {
                    let run = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
                    code = Some(run);
                    at += run;
                    result.push(' ');
                    continue;
                }
                _ => result.push(ch),
            }
        }
        at += ch.len_utf8();
    }
    if quote || code.is_some() || fence.is_some() || !tags.is_empty() {
        return None;
    }
    Some(result)
}

enum Markup<'a> {
    Open(&'a str),
    Close(&'a str),
    Standalone,
}

/// Recognize only tag-shaped openers. This deliberately supports a small
/// balanced subset, without interpreting declarations, entities or external data.
fn markup(text: &str) -> Result<Option<(Markup<'_>, usize)>, ()> {
    let bytes = text.as_bytes();
    if let Some(comment) = text.strip_prefix("<!--") {
        let end = comment.find("-->").ok_or(())? + 7;
        return Ok(Some((Markup::Standalone, end)));
    }
    let closing = bytes.get(1) == Some(&b'/');
    let start = if closing { 2 } else { 1 };
    if !bytes.get(start).is_some_and(u8::is_ascii_alphabetic) {
        return if matches!(bytes.get(1), Some(b'!' | b'?' | b'/')) {
            Err(())
        } else {
            Ok(None)
        };
    }
    let mut end = start;
    while bytes
        .get(end)
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'-' | b':' | b'.'))
    {
        end += 1;
    }
    let name = &text[start..end];
    let Some(after_name) = bytes.get(end) else {
        return Err(()); // An unfinished <context is still ambiguous markup.
    };
    if !after_name.is_ascii_whitespace() && !matches!(after_name, b'/' | b'>') {
        // A name immediately followed by punctuation is not a tag opener:
        // x<y, for example, is a comparison followed by a clause separator.
        // A malformed closing tag still signals unmatched scaffolding.
        return if closing { Err(()) } else { Ok(None) };
    }
    let attributes = end;
    let mut quote = None;
    while let Some(ch) = bytes.get(end).copied() {
        if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
            }
        } else {
            match ch {
                b'\'' | b'"' => quote = Some(ch),
                b'<' => return Err(()),
                b'>' => {
                    let tail = text[attributes..end].trim();
                    let tag = if closing {
                        if !tail.is_empty() {
                            return Err(());
                        }
                        Markup::Close(name)
                    } else if tail.ends_with('/') {
                        Markup::Standalone
                    } else {
                        Markup::Open(name)
                    };
                    return Ok(Some((tag, end + 1)));
                }
                _ => {}
            }
        }
        end += 1;
    }
    Err(())
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
    #[test]
    fn request_scaffolding_preserves_comparisons_and_balanced_content() {
        for comparison in [
            "Fix when x < y, add tests",
            "Fix when x<y, add tests",
            "Fix when x<=y, add tests",
            "Fix when x<y; add tests",
        ] {
            assert_eq!(request_text(comparison).as_deref(), Some(comparison));
        }
        for request in [
            "\"security `CI` build\"\nFix the API crash",
            "\"security\nrefactor\"\nFix the API crash",
            "\"security \\\"CI\\\" build\"\nFix the API crash",
            "`security \"CI\" build`\nFix the API crash",
            "``security `CI` build``\nFix the API crash",
            "```rust\nsecurity\n~~~\nrefactor\n```\nFix the API crash",
            "<context reason=\"a > b\"><tags>security</tags></context>\nFix the API crash",
            "<!-- security > refactor -->\nFix the API crash",
            "<context\n reason='a > b'><tags /></context>\nFix the API crash",
        ] {
            assert_eq!(
                request_text(request).as_deref(),
                Some("Fix the API crash"),
                "{request}"
            );
        }
    }

    #[test]
    fn request_scaffolding_abstains_on_unbalanced_or_mismatched_delimiters() {
        let cases = [
            "Fix the \"login flow. Add a settings page",
            "Fix the 5\" display",
            "Fix the API. `security\nAdd tests",
            "Fix the API. \"security\nAdd tests",
            "Fix the API. <context>security\nAdd tests",
            "Fix the API. <context",
            "Fix the API. <context reason=security",
            "Fix the API. <context note=\"security>\nAdd tests",
            "Fix the API. <context>security</other>\nAdd tests",
            "Fix the API. </context>\nAdd tests",
            "Fix the API.\n```\nsecurity\n~~~\nAdd tests",
            "Fix the API. <!-- security\nAdd tests",
            "Fix the API. ``security`\nAdd tests",
            "Fix the API.\n~~~~\nsecurity\n~~~\nAdd tests",
            "<in-app-browser-context>security\n## My request:\nAdd tests",
        ];
        let failures = cases
            .iter()
            .enumerate()
            .filter_map(|(index, request)| request_text(request).is_some().then_some(index + 1))
            .collect::<Vec<_>>();
        assert!(
            failures.is_empty(),
            "malformed cases returned partial input: {failures:?}"
        );
    }

    #[test]
    fn request_scaffolding_failure_never_falls_through_to_later_user_text() {
        for first in [
            "Fix the 5\" display",
            "Fix <context>\nsecurity",
            "Fix `API",
            "<system-reminder>security",
            "<in-app-browser-context>security\n## My request:\nAdd tests",
        ] {
            let lines = [first,"Add a login screen"].map(|text|serde_json::json!({"type":"event_msg","payload":{"type":"user_message","message":text}}).to_string()).join("\n");
            assert!(first_user_request("codex", &lines).is_none());
        }
    }
}
