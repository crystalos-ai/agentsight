# Suggested session work

The optional host-session `work` field contains closed categories and subjects inferred from the first eligible native user request. It never exports a prompt, title, path, customer name or free-form summary. Labels describe requested work, not actions taken, outcomes, or policy authority.

```json
{"schema_version":"session-work/1","source":"user_prompt","basis":"first_user_request","method":"lexical/v1","categories":["bug","fix","test"],"subjects":["api","testing"]}
```

## Source and limits

The initial producer supports native Claude Code user messages and Codex user-message events or user-role response items. It skips AGENTS.md instructions, known host scaffolding, metadata messages, sidechains, compaction summaries and tool results. Queue entries, last-prompt summaries, database previews, assistant replies, process arguments and tool activity are not classification sources. Forked or subagent Codex sessions are also withheld: their files can include parent requests, and this classifier does not yet prove child ownership. Gemini, Cursor, process-only observations and older stored rows remain usable without work metadata.

The request is held locally and is not serialized as a new session field. Input is limited to 8,192 UTF-8 bytes within the first 512 KiB of native source. An oversized first request, malformed source prefix, missing source or unsupported provider yields no work. A valid request with no recognized terms produces empty arrays. Neither defaults to Chore. Later requests do not replace or expand first-request labels.

Indexed Codex sessions require their native rollout because the SQLite preview is not a reliable first request. The reader accepts absolute `.codex/sessions` or `.codex/archived_sessions` JSONL paths, opens a regular descriptor with `O_NOFOLLOW` and `O_NONBLOCK`, compares `session_meta.payload.id` with the indexed id, and refuses work if descriptor metadata changes during the read. It adds a bounded prefix read without broadening discovery.

The host snapshot still reports at most 25 recent registry rows, including managed and unmanaged sessions visible to that collector. This does not cover every historical session. Existing discovery and source-access limits still apply.

## Vocabulary and interpretation

Categories: Security, Bug, Performance, Edge Case, Quality, Fix, Feature, Refactor, Test, CI, Docs, Chore, Build and Revert. Subjects: Authentication, Authorization, API, UI, Database, Networking, Files, Observability, Dependencies, Build pipeline, Deployment, Testing, Documentation, Configuration and Agent tools. Several labels can apply. A closed subject is a coarse description, not an exact task title.

`lexical/v1` matches English phrases with clause-level negation and word boundaries. It removes fenced code, quoted lines, inline quoted text and XML payloads before matching. Injected category tags cannot supply labels. Authentication alone is a subject, not Security work. “Add a login screen” suggests Feature; “build a dashboard” does not suggest Build; ordinary “run tests afterward” does not suggest Test.

These are suggestions. Lexical matching can miss paraphrases, other languages and complex negation, or misread indirect references outside the supported forms. The first request can become stale as work changes direction. Consumers should expose the basis and method and keep missing metadata and empty arrays distinct from successful classification.

## Reproduce the wire fixture

The synthetic files in `collector/tests/fixtures/session-work/` contain both native providers, host scaffolding, later task changes and planted secret canaries. The collector regression runs them through the actual native parser, materialized session view and host projection. It checks exact labels and prevents raw request and command text from leaving the process.

```sh
rtk proxy env AGENTSIGHT_WORK_FIXTURE_OUTPUT=/absolute/output/host-sessions.json cargo test --manifest-path collector/Cargo.toml source_fixtures_produce_closed_work_metadata -- --nocapture
rtk cargo test --manifest-path ext/session/Cargo.toml
rtk cargo test --manifest-path ext/analysis/Cargo.toml session_work
rtk cargo test --manifest-path ext/analysis/Cargo.toml indexed_codex_first_request
rtk cargo test --manifest-path agentsight-protocol/Cargo.toml --features bridge
rtk cargo test --manifest-path collector/Cargo.toml host_sessions
```

The generated wire JSON is synthetic integration evidence, not a real-user observation or a deployment claim.
