// SPDX-License-Identifier: MIT
//! Conservative English lexical suggestions, never policy or verified outcomes.
use agentsight_protocol::bridge::{
    SessionWork, WorkBasis, WorkCategory as C, WorkMethod, WorkSchema, WorkSource, WorkSubject as S,
};

/// Only call with a source-native first user request. No command/preview fallback.
pub fn classify(request: &str) -> Option<SessionWork> {
    let text = agent_session::request_text(request)?;
    let mut categories = Vec::new();
    let mut subjects = Vec::new();
    // Sentence/clause scope prevents negated follow-up work becoming a suggestion.
    let lower = text
        .to_lowercase()
        .replace("don't", "do not")
        .replace("don’t", "do not")
        .replace(" but ", ";")
        .replace(" instead ", ";");
    for clause in lower
        .split(['.', ';', '\n', '!', '?', ','])
        .flat_map(positive_clauses)
    {
        let tokens = clause
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let has = |phrases: &[&str]| {
            phrases
                .iter()
                .any(|phrase| positive_phrase(&tokens, phrase))
        };
        let change = has(&[
            "add",
            "implement",
            "create",
            "introduce",
            "develop",
            "enable",
        ]);
        let repair = has(&["fix", "repair", "resolve", "correct", "patch"]);
        let analyze = has(&[
            "investigate",
            "debug",
            "diagnose",
            "reproduce",
            "find why",
            "find out why",
            "find the cause",
            "find the root cause",
            "why does",
            "why do",
            "why is",
            "why are",
        ]);
        let docs = has(&[
            "document",
            "documentation",
            "readme",
            "write docs",
            "update docs",
            "edit docs",
        ]);
        let tests = ((!has(&["run", "execute", "verify", "validate"])
            || has(&["write", "add", "create", "fix", "repair", "flaky"]))
            && has(&[
                "write tests",
                "add tests",
                "create tests",
                "test suite",
                "test coverage",
                "regression test",
                "regression tests",
                "unit test",
                "unit tests",
                "integration test",
                "integration tests",
                "test harness",
                "flaky tests",
                "flaky test",
            ]))
            || ((analyze || repair)
                && has(&["tests fail", "test fails", "failing tests", "failing test"]));
        let ci = has(&[
            "ci",
            "continuous integration",
            "github actions",
            "workflow yaml",
        ]);
        let build = has(&[
            "compiler",
            "compilation",
            "compile error",
            "compile errors",
            "build failure",
            "build fails",
            "build error",
            "build errors",
            "build system",
            "build pipeline",
            "bundler",
            "webpack",
            "vite config",
            "cargo build",
        ]);
        let revert = has(&[
            "revert",
            "undo commit",
            "undo change",
            "undo changes",
            "roll back change",
            "roll back commit",
        ]);
        let maintenance = !revert
            && has(&[
                "chore",
                "housekeeping",
                "update dependencies",
                "upgrade dependencies",
                "bump dependencies",
                "dependency update",
                "dependency upgrade",
                "bump version",
            ]);
        let refactor = has(&[
            "refactor",
            "refactoring",
            "restructure",
            "extract helper",
            "extract helpers",
        ]);
        let feature = change && !docs && !tests && !ci && !build && !maintenance && !refactor
            || has(&[
                "new feature",
                "feature request",
                "build a dashboard",
                "build a page",
                "build an app",
                "build a ui",
            ]);
        let bug = !docs
            && (repair || analyze || has(&["bug", "bugs"]))
            && has(&[
                "bug",
                "bugs",
                "crash",
                "crashes",
                "regression",
                "incorrect",
                "broken",
                "fail",
                "fails",
                "failing",
                "defect",
            ]);
        for (label, matched) in [
            (
                C::Security,
                !docs
                    && has(&[
                        "security audit",
                        "security review",
                        "security flaw",
                        "security vulnerability",
                        "security issue",
                        "vulnerability",
                        "vulnerabilities",
                        "harden",
                        "hardening",
                        "cve",
                        "xss",
                        "csrf",
                        "sql injection",
                    ]),
            ),
            (C::Bug, bug),
            (
                C::Performance,
                has(&[
                    "performance",
                    "latency",
                    "slow",
                    "optimize",
                    "optimise",
                    "memory leak",
                    "throughput",
                    "bundle size",
                ]),
            ),
            (
                C::EdgeCase,
                has(&[
                    "edge case",
                    "edge cases",
                    "boundary condition",
                    "boundary conditions",
                    "null handling",
                    "empty input",
                    "race condition",
                    "race conditions",
                    "overflow",
                    "empty uploads",
                    "zero length",
                ]),
            ),
            (
                C::Quality,
                has(&[
                    "code review",
                    "code quality",
                    "readability",
                    "lint",
                    "linting",
                    "dead code",
                    "technical debt",
                ]),
            ),
            (C::Fix, repair),
            (C::Feature, feature),
            (C::Refactor, refactor),
            (C::Test, tests),
            (C::Ci, ci),
            (C::Docs, docs),
            (C::Chore, maintenance),
            (C::Build, build),
            (C::Revert, revert),
        ] {
            if matched && !categories.contains(&label) {
                categories.push(label);
            }
        }
        for (subject, matched) in [
            (
                S::Authentication,
                has(&[
                    "authentication",
                    "login",
                    "log in",
                    "sign in",
                    "signin",
                    "oauth",
                    "password",
                ]),
            ),
            (
                S::Authorization,
                has(&[
                    "authorization",
                    "permission",
                    "permissions",
                    "access control",
                    "rbac",
                ]),
            ),
            (
                S::Api,
                has(&[
                    "api",
                    "endpoint",
                    "endpoints",
                    "rest service",
                    "rest services",
                    "restful",
                    "graphql",
                ]),
            ),
            (
                S::Ui,
                has(&[
                    "ui",
                    "frontend",
                    "screen",
                    "page",
                    "dashboard",
                    "button",
                    "layout",
                    "css",
                    "component",
                ]),
            ),
            (
                S::Database,
                has(&[
                    "database",
                    "sql",
                    "postgres",
                    "postgresql",
                    "sqlite",
                    "query",
                    "migration",
                ]),
            ),
            (
                S::Networking,
                has(&[
                    "network",
                    "networking",
                    "dns",
                    "tcp",
                    "socket",
                    "http",
                    "proxy",
                ]),
            ),
            (
                S::Files,
                has(&["file", "files", "filesystem", "directory", "directories"]),
            ),
            (
                S::Observability,
                has(&[
                    "logging",
                    "logs",
                    "metrics",
                    "tracing",
                    "telemetry",
                    "observability",
                ]),
            ),
            (
                S::Dependencies,
                has(&["dependency", "dependencies", "package", "packages"]),
            ),
            (S::BuildPipeline, build || ci),
            (
                S::Deployment,
                has(&["deployment", "deploy", "rollout", "kubernetes", "docker"]),
            ),
            (S::Testing, tests),
            (S::Documentation, docs),
            (
                S::Configuration,
                has(&[
                    "configuration",
                    "config",
                    "settings",
                    "environment variable",
                    "environment variables",
                ]),
            ),
            (
                S::AgentTools,
                has(&[
                    "agent tool",
                    "agent tools",
                    "mcp",
                    "tool call",
                    "tool calls",
                    "coding agent",
                ]),
            ),
        ] {
            if matched && !subjects.contains(&subject) {
                subjects.push(subject);
            }
        }
    }
    Some(SessionWork {
        schema_version: WorkSchema::V1,
        source: WorkSource::UserPrompt,
        basis: WorkBasis::FirstUserRequest,
        method: WorkMethod::LexicalV1,
        categories,
        subjects,
    })
}

// Split separate positive asks, preserving the scope of a preceding negation.
fn positive_clauses(clause: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = clause;
    while let Some((left, right)) = rest.split_once(" and ") {
        if left.split_whitespace().any(|word| {
            matches!(
                word,
                "not" | "no" | "never" | "without" | "avoid" | "skip" | "except"
            )
        }) {
            break;
        }
        parts.push(left);
        rest = right;
    }
    parts.push(rest);
    parts
}

fn positive_phrase(tokens: &[&str], phrase: &str) -> bool {
    let needle = phrase.split_whitespace().collect::<Vec<_>>();
    tokens
        .windows(needle.len())
        .enumerate()
        .any(|(index, words)| {
            words == needle
                && !tokens[..index].iter().any(|word| {
                    matches!(
                        *word,
                        "not" | "no" | "never" | "without" | "avoid" | "skip" | "except"
                    )
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_category_has_an_explicit_positive_request() {
        for (text, expected) in [
            ("Audit the security vulnerability", C::Security),
            ("Investigate the API bug", C::Bug),
            ("Improve database performance", C::Performance),
            ("Handle the empty input edge case", C::EdgeCase),
            ("Review code quality", C::Quality),
            ("Fix the login issue", C::Fix),
            ("Add a login screen", C::Feature),
            ("Refactor the API", C::Refactor),
            ("Add regression tests for the API", C::Test),
            ("Repair CI", C::Ci),
            ("Update documentation", C::Docs),
            ("Upgrade dependencies", C::Chore),
            ("Fix the compiler error", C::Build),
            ("Revert the last commit", C::Revert),
        ] {
            assert!(
                classify(text).unwrap().categories.contains(&expected),
                "{text}"
            );
        }
    }
    #[test]
    fn overlapping_labels_are_suggestions_with_explicit_basis() {
        let work =
            classify("Fix the API bug and add regression tests. Harden authorization.").unwrap();
        assert_eq!(work.categories, vec![C::Bug, C::Fix, C::Test, C::Security]);
        assert_eq!(work.subjects, vec![S::Api, S::Testing, S::Authorization]);
        assert_eq!(work.basis, WorkBasis::FirstUserRequest);
    }
    #[test]
    fn negation_scaffolding_and_routine_verification_do_not_add_work() {
        let work = classify("Fix the API bug, don't refactor. Run tests afterward. <categories>security</categories>\n> build failure\n```\nCI\n```").unwrap();
        assert_eq!(work.categories, vec![C::Bug, C::Fix]);
        assert_eq!(work.subjects, vec![S::Api]);
        assert_eq!(
            classify("Add a login screen").unwrap().categories,
            vec![C::Feature]
        );
        assert_eq!(
            classify("Build a dashboard").unwrap().categories,
            vec![C::Feature]
        );
        assert_eq!(
            classify("Add a capacity display").unwrap().categories,
            vec![C::Feature]
        );
        assert!(
            classify("No bug. Do not change security or write tests")
                .unwrap()
                .categories
                .is_empty()
        );
        assert!(classify("Please continue").unwrap().categories.is_empty());
        assert!(
            classify("Fix \"security CI bug\" wording in documentation")
                .unwrap()
                .categories
                .iter()
                .all(|c| matches!(c, C::Fix | C::Docs))
        );
    }
    #[test]
    fn separate_asks_and_routine_checks_keep_their_scope() {
        assert_eq!(
            classify("Add a login screen and write unit tests")
                .unwrap()
                .categories,
            vec![C::Feature, C::Test]
        );
        assert_eq!(
            classify("Add a login screen. Run the unit tests afterward.")
                .unwrap()
                .categories,
            vec![C::Feature]
        );
        assert!(
            classify("Do not refactor and add tests")
                .unwrap()
                .categories
                .is_empty()
        );
        assert_eq!(
            classify("Document the security vulnerability")
                .unwrap()
                .categories,
            vec![C::Docs]
        );
    }

    #[test]
    fn ambiguous_reference_words_do_not_invent_work_or_api_subjects() {
        for text in [
            "Run the rest of the tests.",
            "Which operating systems does this package support?",
            "Find the files that support the failing test.",
            "Find the explanation of why validation fails for invalid input.",
        ] {
            let work = classify(text).unwrap();
            assert!(work.categories.is_empty(), "{text}: {:?}", work.categories);
            assert!(!work.subjects.contains(&S::Api), "{text}");
        }
    }

    #[test]
    fn explicit_causal_investigations_and_qualified_features_remain_supported() {
        let work = classify("Find why the rest of the tests fail.").unwrap();
        assert_eq!(work.categories, vec![C::Bug, C::Test]);
        assert_eq!(work.subjects, vec![S::Testing]);
        for text in [
            "Why does the API crash on startup?",
            "Why is login failing?",
            "Find out why the API crashes.",
            "Find the root cause of the API crash.",
            "Find the API bug.",
            "Investigate failing tests.",
        ] {
            assert!(
                classify(text).unwrap().categories.contains(&C::Bug),
                "{text}"
            );
        }
        assert_eq!(
            classify("Add support for OAuth login.").unwrap().categories,
            vec![C::Feature]
        );
        for text in [
            "Add a REST API.",
            "Investigate the REST service crash.",
            "Document the RESTful interface.",
        ] {
            assert!(classify(text).unwrap().subjects.contains(&S::Api), "{text}");
        }
    }

    #[test]
    fn independent_review_cases() {
        let cases: &[(&str, &[C], &[C])] = &[
            (
                "Add a login screen. Run tests when done.",
                &[C::Feature],
                &[C::Security, C::Test, C::Build],
            ),
            (
                "Do not refactor or add features. Fix the null pointer bug in the API.",
                &[C::Fix, C::Bug],
                &[C::Refactor, C::Feature],
            ),
            (
                "Investigate slow database queries; do not change behavior yet.",
                &[C::Performance],
                &[C::Fix],
            ),
            (
                "Document the authentication API.",
                &[C::Docs],
                &[C::Security],
            ),
            ("Revert the previous dependency upgrade.", &[C::Revert], &[]),
            (
                "Handle empty uploads and zero-length files.",
                &[C::EdgeCase],
                &[],
            ),
            ("Add tests for expired tokens.", &[C::Test], &[C::Security]),
            ("Update GitHub Actions to use Node 22.", &[C::Ci], &[]),
            ("Fix the CI build failure.", &[C::Fix, C::Ci, C::Build], &[]),
            ("Reduce bundle size.", &[C::Performance], &[]),
            ("Continue.", &[], &[C::Chore]),
            ("Explain this code.", &[], &[C::Chore]),
            (
                "Don't fix anything; explain why tests fail.",
                &[],
                &[C::Fix],
            ),
        ];
        let mut failures = Vec::new();
        for (index, (text, required, forbidden)) in cases.iter().enumerate() {
            let work = classify(text).unwrap();
            let pass = required.iter().all(|c| work.categories.contains(c))
                && forbidden.iter().all(|c| !work.categories.contains(c));
            println!(
                "case {}: {:?}; {}",
                index + 1,
                work.categories,
                if pass { "PASS" } else { "FAIL" }
            );
            if !pass {
                failures.push(index + 1);
            }
        }
        assert!(failures.is_empty(), "review case failures: {failures:?}");
    }
}
