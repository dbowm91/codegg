//! C004 — the domain's terminality authority, and the session projection
//! that has to agree with it.
//!
//! `JobState::is_terminal()` in `crates/codegg-core/src/jobs/mod.rs` is the
//! one authority on whether a job is finished: `Completed`, `Failed`,
//! `Cancelled`, `TimedOut` and `Expired`. `Interrupted` is **not** terminal,
//! because an interrupted attempt may still be requeued.
//!
//! The session projection used to keep its own list of "terminal" states,
//! and that list included `Interrupted` — a terminality claim that
//! contradicted the domain. The projection's queries are read here and
//! checked against the domain, because a query's state list is a set of rows
//! selected rather than a value returned: it is observable nowhere else.

use codegg_core::jobs::JobState;

/// The session projection's source.
const PROJECTION_SOURCE: &str = include_str!("../../src/core/daemon_refresh.rs");

/// Every `JobState` variant.
///
/// The list is written out so the test can compute the domain's own answer
/// for each variant with the domain's own predicate, on a build that has the
/// predicate and nothing else.
const JOB_STATE_VARIANTS: [JobState; 10] = [
    JobState::Scheduled,
    JobState::Queued,
    JobState::Running,
    JobState::Completed,
    JobState::Failed,
    JobState::Cancelled,
    JobState::TimedOut,
    JobState::Interrupted,
    JobState::Blocked,
    JobState::Expired,
];

/// The states the domain calls terminal, in the domain's own spelling.
fn domain_terminal_states() -> Vec<String> {
    JOB_STATE_VARIANTS
        .iter()
        .filter(|state| state.is_terminal())
        .map(|state| state.as_str().to_string())
        .collect()
}

/// The states the domain does not call terminal, in the domain's own
/// spelling.
fn domain_non_terminal_states() -> Vec<String> {
    JOB_STATE_VARIANTS
        .iter()
        .filter(|state| !state.is_terminal())
        .map(|state| state.as_str().to_string())
        .collect()
}

/// The spelling the domain uses for a variant name: `TimedOut` is
/// `timed_out`.
fn domain_spelling(variant_name: &str) -> String {
    let mut spelling = String::new();
    for character in variant_name.chars() {
        if character.is_uppercase() && !spelling.is_empty() {
            spelling.push('_');
        }
        spelling.extend(character.to_lowercase());
    }
    spelling
}

/// The `states:` argument of the query bound to `binding`.
fn states_argument(source: &str, binding: &str) -> String {
    let start = source
        .find(&format!("let {binding} ="))
        .unwrap_or_else(|| panic!("the projection binds `{binding}`"));
    let after = &source[start..];
    let marker = after
        .find("states:")
        .unwrap_or_else(|| panic!("the `{binding}` query names its state list"));
    let rest = after[marker + "states:".len()..].trim_start();
    if rest.starts_with("vec![") {
        let end = rest.find(']').expect("a closed state list") + 1;
        rest[..end].to_string()
    } else {
        let end = rest.find(',').unwrap_or(rest.len());
        rest[..end].trim().to_string()
    }
}

/// The states a `states:` argument selects.
///
/// An argument that derives its states from the domain selects them by
/// construction; an argument that writes them out selects exactly the ones
/// it names.
fn selected_states(argument: &str) -> Vec<String> {
    let mut states = if argument.contains("non_terminal_states()") {
        domain_non_terminal_states()
    } else if argument.contains("terminal_states()") {
        domain_terminal_states()
    } else {
        let mut written: Vec<String> = Vec::new();
        let mut rest = argument;
        while let Some(at) = rest.find("JobState::") {
            rest = &rest[at + "JobState::".len()..];
            let name: String = rest
                .chars()
                .take_while(|character| character.is_alphanumeric() || *character == '_')
                .collect();
            written.push(domain_spelling(&name));
        }
        written
    };
    states.sort();
    states
}

#[test]
fn the_session_projection_queries_agree_with_the_domain_terminality() {
    let terminal = selected_states(&states_argument(PROJECTION_SOURCE, "terminal_jobs"));
    let mut expected_terminal = domain_terminal_states();
    expected_terminal.sort();
    assert_eq!(
        terminal, expected_terminal,
        "the projection's terminal-jobs query must select exactly the states \
         the domain calls terminal"
    );

    let live = selected_states(&states_argument(PROJECTION_SOURCE, "live_jobs"));
    let mut expected_live = domain_non_terminal_states();
    expected_live.sort();
    assert_eq!(
        live, expected_live,
        "the projection's live-jobs query must select exactly the states the \
         domain does not call terminal"
    );

    // The premise of both assertions above: the domain does not call an
    // interrupted job terminal, so it belongs with the live jobs.
    assert!(
        !JobState::Interrupted.is_terminal(),
        "the domain does not call an interrupted job terminal"
    );
    assert!(
        !domain_terminal_states().contains(&"interrupted".to_string()),
        "an interrupted job is not among the domain's terminal states"
    );
    assert!(
        domain_non_terminal_states().contains(&"interrupted".to_string()),
        "an interrupted job is among the states the domain does not call terminal"
    );
}
