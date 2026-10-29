// SPDX-License-Identifier: MIT
// Copyright (c) 2026 eunomia-bpf org.

//! Data types for agent session representation.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::{discover_session_files, parse_session_file};

/// The most session data one cache keeps with events, as the JSON length of
/// its parsed sessions, and the most one parse keeps. The largest of 541 real
/// Claude transcripts parses to 10 MB, and 3 pass 8 MiB; a transcript written
/// to be costly (Bash calls naming many paths) gives about five times its
/// bytes, takes about three times that in memory, and is kept in up to three
/// copies. A session past the budget keeps its totals without its events, and
/// is marked truncated, until its file changes.
pub(crate) const MAX_CACHED_SESSION_BYTES: usize = 8 * 1024 * 1024;

/// Token usage statistics for a model or session.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
}

impl TokenUsage {
    pub(crate) fn add(
        &mut self,
        input: i64,
        output: i64,
        cache_creation: i64,
        cache_read: i64,
        total: i64,
    ) {
        self.input_tokens += input;
        self.output_tokens += output;
        self.cache_creation_tokens += cache_creation;
        self.cache_read_tokens += cache_read;
        self.total_tokens += if total > 0 {
            total
        } else {
            input + output + cache_creation + cache_read
        };
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPrompt {
    pub index: usize,
    pub ts_ms: Option<i64>,
    pub text_hash: String,
    /// Full source-visible prompt text for the authorized session detail API.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    pub preview: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag: String,
    /// Source-visible semantic responsibility path after applying this prompt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_path: Vec<String>,
}

impl UserPrompt {
    pub fn prompt_key(&self) -> String {
        format!("{}:{}", self.index, self.text_hash)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPath {
    /// Path as recorded by the native agent. Consumers resolve relative paths
    /// against the session cwd and must reject paths outside their scope.
    pub path: String,
    /// One of read, write, create, delete, rename_from, or rename.
    pub access: String,
    /// Source path for a rename. Kept on the destination so one tool event can
    /// carry several independent rename pairs without positional guessing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEvent {
    pub ts_ms: Option<i64>,
    pub prompt_index: usize,
    pub tool_name: String,
    pub category: String,
    pub command: String,
    pub command_name: String,
    pub effect: String,
    pub process_chain: Vec<String>,
    pub status: String,
    pub path_groups: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<ToolPath>,
    pub domains: Vec<String>,
    pub call_id: Option<String>,
    /// Exact nonempty `input.skill` on a source-native Skill tool call.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub invoked_skill: String,
    /// Exact source-recorded skill scope active at this tool invocation.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub skill: String,
    /// Source-visible semantic responsibility path active at this operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub ts_ms: Option<i64>,
    pub prompt_index: usize,
    pub model: String,
    /// Source-native completion identity used to merge split JSONL records.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_id: String,
    pub text_hash: String,
    /// Full source-visible response text for the authorized session detail API.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    pub preview: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag: String,
    /// Source-native response lifecycle when the agent records one explicitly.
    /// Examples are `commentary`, `final_answer`, and `assistant_message`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub response_phase: String,
    /// Exact source-recorded skill scope active when this response began.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub skill: String,
    /// Source-visible semantic responsibility path active at this response.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_path: Vec<String>,
}

/// Latest source-recorded coding plan entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStep {
    pub step: String,
    pub status: String,
}

impl LlmResponse {
    pub fn token_components(&self) -> Vec<(&'static str, u64)> {
        const MAX_REPORTED_TOKEN_COMPONENT: u64 = 10_000_000;
        const MAX_ESTIMATED_TOKEN_COMPONENT: u64 = 2_000_000;
        let mut out = Vec::new();
        if (1..=MAX_REPORTED_TOKEN_COMPONENT).contains(&self.input_tokens) {
            out.push(("input", self.input_tokens));
        }
        if (1..=MAX_REPORTED_TOKEN_COMPONENT).contains(&self.output_tokens) {
            out.push(("output", self.output_tokens));
        }
        if (1..=MAX_REPORTED_TOKEN_COMPONENT).contains(&self.cache_tokens) {
            out.push(("cache", self.cache_tokens));
        }
        if out.is_empty() && (1..=MAX_ESTIMATED_TOKEN_COMPONENT).contains(&self.total_tokens) {
            out.push(("estimate", self.total_tokens));
        }
        if out.is_empty() {
            out.push(("unknown", 1));
        }
        out
    }
}

/// Vendor-neutral interaction events extracted from an agent-native transcript.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionEvents {
    pub prompts: Vec<UserPrompt>,
    pub tools: Vec<ToolEvent>,
    pub llm_responses: Vec<LlmResponse>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plan: Vec<PlanStep>,
}

/// A parsed agent session with metadata, token usage, and tool invocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub agent_type: String,
    pub session_id: String,
    pub conversation_id: Option<String>,
    pub display_id: String,
    pub path: PathBuf,
    pub updated: SystemTime,
    pub start_timestamp_ms: Option<u64>,
    pub end_timestamp_ms: Option<u64>,
    pub model: Option<String>,
    pub usage: TokenUsage,
    pub model_usage: BTreeMap<String, TokenUsage>,
    pub tools: BTreeMap<String, usize>,
    pub files: BTreeMap<String, usize>,
    pub prompt_preview: Option<String>,
    pub duration_ms: u64,
    pub cwd: Option<String>,
    pub last_message_at: Option<String>,
    /// Vendor-neutral interaction events extracted from agent-native transcripts.
    #[serde(default)]
    pub events: SessionEvents,
    /// Some of the session was not read or kept: a line past the parser's
    /// limit, the end of a file past its read limit, or events and keys past
    /// a budget. Totals then count only what was read.
    #[serde(default)]
    pub truncated: bool,
}

/// A candidate session file discovered on disk.
#[derive(Debug, Clone)]
pub struct SessionCandidate {
    pub agent: &'static str,
    pub path: PathBuf,
    pub updated: SystemTime,
}

/// Statistics about a session directory.
#[derive(Debug, Clone)]
pub struct SessionDirStat {
    pub agent: &'static str,
    pub dir: PathBuf,
    pub sessions: usize,
    pub bytes: u64,
}

/// Cache for discovered and parsed sessions.
#[derive(Default)]
pub struct SessionCache {
    entries: HashMap<PathBuf, CacheEntry>,
    cached_sessions: Vec<AgentSession>,
    last_refresh: Option<Instant>,
    last_limit: usize,
    last_excluded_agents: Vec<String>,
}

struct CacheEntry {
    mtime: SystemTime,
    session: Option<AgentSession>,
    /// The JSON length of `session`, its share of the budget.
    bytes: usize,
    pinned: bool,
}

impl CacheEntry {
    fn parse(candidate: &SessionCandidate, pinned: bool) -> Self {
        let session = parse_session_file(candidate);
        // Only a modification time before 1970 fails to serialize; such a
        // session cannot fit.
        let bytes = session.as_ref().map_or(0, crate::parser::json_len);
        Self {
            mtime: candidate.updated,
            session,
            bytes,
            pinned,
        }
    }
}

impl SessionCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse one known transcript without widening the bounded discovery scan.
    /// The parsed session is reused until the file's modification time changes.
    pub fn parse_path_cached(&mut self, path: &Path) -> Option<AgentSession> {
        let Some(candidate) = crate::session_candidate_from_path(path) else {
            self.entries.remove(path);
            return None;
        };
        if let Some(entry) = self.entries.get_mut(path)
            && entry.mtime == candidate.updated
        {
            entry.pinned = true;
            let session = entry.session.clone();
            self.trim_pinned_details();
            return session;
        }
        let entry = CacheEntry::parse(&candidate, true);
        let parsed = entry.session.clone();
        self.entries.insert(path.to_path_buf(), entry);
        self.trim_pinned_details();
        parsed
    }

    fn trim_pinned_details(&mut self) {
        const MAX_PINNED_DETAILS: usize = 8;
        let mut pinned = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.pinned)
            .map(|(path, entry)| (path.clone(), entry.mtime))
            .collect::<Vec<_>>();
        if pinned.len() <= MAX_PINNED_DETAILS {
            return;
        }
        let overflow = pinned.len() - MAX_PINNED_DETAILS;
        pinned.sort_by_key(|(_, mtime)| *mtime);
        for (path, _) in pinned.into_iter().take(overflow) {
            self.entries.remove(&path);
        }
    }

    pub fn discover_cached(&mut self, limit: usize, max_age: Duration) -> Vec<AgentSession> {
        self.discover_cached_excluding(limit, max_age, &[])
    }

    pub fn discover_cached_excluding(
        &mut self,
        limit: usize,
        max_age: Duration,
        excluded_agents: &[&str],
    ) -> Vec<AgentSession> {
        let target = limit.clamp(1, 25);
        let mut excluded_agents = excluded_agents
            .iter()
            .map(|agent| (*agent).to_string())
            .collect::<Vec<_>>();
        excluded_agents.sort();
        if self.last_limit < target
            || self.last_excluded_agents != excluded_agents
            || self
                .last_refresh
                .is_none_or(|last| last.elapsed() >= max_age)
        {
            self.refresh(target, &excluded_agents);
        }
        self.cached_sessions.iter().take(target).cloned().collect()
    }

    fn refresh(&mut self, limit: usize, excluded_agents: &[String]) {
        let mut candidates = discover_session_files();
        candidates
            .retain(|candidate| !excluded_agents.iter().any(|agent| agent == candidate.agent));
        self.refresh_from(candidates, limit, excluded_agents);
    }

    fn refresh_from(
        &mut self,
        mut candidates: Vec<SessionCandidate>,
        limit: usize,
        excluded_agents: &[String],
    ) {
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.updated));
        let target = limit.clamp(1, 25);
        let mut live_paths = HashSet::new();
        let mut sessions = Vec::new();
        let mut seen = HashSet::new();
        let mut kept_bytes = 0usize;

        for candidate in candidates
            .into_iter()
            .take(target.saturating_mul(3).clamp(10, 75))
        {
            live_paths.insert(candidate.path.clone());
            let entry = self
                .entries
                .entry(candidate.path.clone())
                .and_modify(|entry| {
                    if entry.mtime != candidate.updated {
                        *entry = CacheEntry::parse(&candidate, false);
                    }
                })
                .or_insert_with(|| CacheEntry::parse(&candidate, false));
            if let Some(session) = entry.session.as_mut() {
                match kept_bytes.checked_add(entry.bytes) {
                    Some(total) if total <= MAX_CACHED_SESSION_BYTES => kept_bytes = total,
                    _ => {
                        session.events = SessionEvents::default();
                        session.truncated = true;
                        entry.bytes = crate::parser::json_len(session);
                    }
                }
            }
            if let Some(session) = entry.session.clone()
                && seen.insert(session.display_id.clone())
            {
                sessions.push(session);
                if sessions.len() >= target {
                    break;
                }
            }
        }
        self.entries
            .retain(|path, entry| live_paths.contains(path) || (entry.pinned && path.is_file()));
        self.cached_sessions = sessions;
        self.last_refresh = Some(Instant::now());
        self.last_limit = target;
        self.last_excluded_agents = excluded_agents.to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover_session_files_in_home;
    use std::fs;

    /// 480 KiB of Bash calls, each naming 1,500 distinct paths: a transcript
    /// that parses to about five times its bytes.
    fn costly_transcript() -> String {
        let mut out = String::new();
        for call in 0.. {
            if out.len() >= 480 * 1024 {
                break;
            }
            let paths: String = (0..1500).map(|n| format!(" /w/a{call}_{n}")).collect();
            out += &format!(
                r#"{{"type":"assistant","message":{{"model":"m","content":[{{"type":"tool_use","id":"t{call}","name":"Bash","input":{{"command":"cat{paths}"}}}}],"usage":{{"input_tokens":1,"output_tokens":1}}}}}}"#
            );
            out.push('\n');
        }
        out
    }

    /// The cache keeps sessions' events, newest first, only while their total
    /// fits its budget: costly transcripts past it keep their totals without
    /// events, and an older honest session still fits after them.
    #[test]
    fn the_cache_keeps_sessions_only_within_its_budget() {
        let home = std::env::temp_dir().join(format!("agentsight-budget-{}", std::process::id()));
        let project = home.join(".claude/projects/-w");
        fs::create_dir_all(&project).expect("project");
        fs::write(
            project.join("honest.jsonl"),
            concat!(
                r#"{"type":"user","message":{"content":"fix the build"}}"#,
                "\n",
                r#"{"type":"assistant","message":{"model":"m","content":[{"type":"text","text":"done"}],"usage":{"input_tokens":1,"output_tokens":1}}}"#,
                "\n"
            ),
        )
        .expect("honest session");
        let costly = costly_transcript();
        for n in 1..=4 {
            std::thread::sleep(Duration::from_millis(20));
            fs::write(project.join(format!("costly-{n}.jsonl")), &costly).expect("costly session");
        }

        let mut cache = SessionCache::new();
        cache.refresh_from(discover_session_files_in_home(&home), 25, &[]);

        let sessions = &cache.cached_sessions;
        let with_events: usize = cache
            .entries
            .values()
            .filter_map(|e| e.session.as_ref().filter(|s| !s.events.tools.is_empty()).map(|_| e.bytes))
            .sum();
        assert!(with_events <= MAX_CACHED_SESSION_BYTES);
        let shown: Vec<(&str, bool)> = sessions
            .iter()
            .map(|s| (s.path.file_name().unwrap().to_str().unwrap(), s.events.tools.is_empty() && s.events.llm_responses.is_empty()))
            .collect();
        assert_eq!(shown.len(), 5, "every session is shown: {shown:?}");
        assert_eq!(shown[0], ("costly-4.jsonl", false), "the newest first, with events");
        assert_eq!(shown[4], ("honest.jsonl", false), "an honest session after them keeps its events");
        let trimmed: Vec<_> = sessions.iter().filter(|s| s.events.llm_responses.is_empty()).collect();
        assert!(!trimmed.is_empty(), "a costly transcript past the budget: {shown:?}");
        for session in trimmed {
            assert!(session.truncated);
            assert_eq!(session.tools["Bash"], sessions[0].tools["Bash"], "its totals are kept");
        }
        fs::remove_dir_all(home).expect("remove scratch directory");
    }
}
