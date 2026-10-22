//! C001 negative guards: the M005 writer-containment restricted-profile
//! contract must stay honest about what CodeGG can write.
//!
//! C001 is a **decision** milestone. It creates no permit, no fence and no
//! runtime behaviour, and it must not enable any writer. What it does is
//! make an undecided writer visible, and keep the declared dispositions
//! consistent with the live tool catalog.
//!
//! These tests are the executable half of that contract:
//!
//! * a prospective shared mutator that claims to be read-only is refused
//!   (a writer-category tool may not declare a read-only effect class);
//! * a tool may only enter the read-only programme palette by being
//!   reviewed into an explicit allowlist;
//! * the legacy isolated mutator keeps working -- C001 changed no runtime
//!   behaviour, and the default catalog still dispatches its writers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use codegg::tool::broker::ToolBroker;
use codegg::tool::contract::ToolEffectClass;
use codegg::tool::{ToolCategory, ToolRegistry, ToolRegistryOptions};

/// The catalog the read-only palette is reviewed against.
///
/// The palette is a set of **tool types** that may declare a read-only effect
/// class, so it has to be read from a catalog where those tools are *live*.
/// A profile that requires filesystem containment attaches a disabled stub for
/// the LSP tools unless the daemon supplies an M004-scoped service, and a
/// disabled stub carries no contract of its own — its effect class is the
/// conservative legacy default, not `ReadOnly`. Reading the palette from a
/// constrained registry would make the reviewed set flip with the profile,
/// and `lsp_read` would read as "no longer read-only" when it is exactly as
/// read-only as it was.
fn reviewed_catalog() -> (ToolRegistry, ToolBroker) {
    let registry = ToolRegistry::with_options(ToolRegistryOptions {
        sandbox_profile: Some(codegg_core::approval::SandboxProfile::FullHost),
        ..Default::default()
    });
    let broker = ToolBroker::new(&registry);
    (registry, broker)
}

/// The C001-reviewed set of tools that may declare a read-only effect class
/// and therefore become program-callable inside a Tool Program.
///
/// A false read-only assertion is a privilege escalation: it moves a writer
/// into the read-only palette. The palette check itself is fail-closed
/// (`resolve_contract_snapshot` requires an explicit read-only effect class),
/// so a new tool cannot self-declare its way in without appearing here --
/// and appearing here is the review gate.
///
/// This list is the **live catalog**'s read-only set, reviewed. It is
/// asserted exactly, so a tool that joins or leaves the set fails the test
/// until the change is reviewed rather than silently absorbed.
const REVIEWED_READ_ONLY_PALETTE: &[&str] = &[
    "diff",
    "git_query",
    "git_read",
    "glob",
    "grep",
    "list",
    "lsp_read",
    "read",
    "repo_search",
    "tool_program",
];

/// Dispositions that would assert the manifest grants authority. The C001
/// manifest is a contract: it records what the restricted profile *would*
/// do, and grants nothing.
const AUTHORITY_VOCABULARY: &[&str] = &[
    "enabled",
    "active",
    "permitted",
    "allowed",
    "granted",
    "open",
];

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/writer-disposition.toml")
}

/// Read the disposition manifest as `(tool, scope, disposition)` triples.
fn load_manifest() -> Vec<(String, String, String)> {
    let raw = std::fs::read_to_string(manifest_path())
        .expect("docs/writer-disposition.toml must exist for the C001 contract");
    let parsed: toml::Value = raw
        .parse()
        .expect("the disposition manifest must be valid TOML");

    let mut out = Vec::new();
    for entry in parsed
        .get("root")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
    {
        let field = |name: &str| {
            entry
                .get(name)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        let tool = field("tool");
        if tool.is_empty() {
            continue;
        }
        out.push((tool, field("scope"), field("disposition")));
    }
    out
}

fn writer_categories() -> [ToolCategory; 2] {
    [ToolCategory::Mutating, ToolCategory::ShellExec]
}

fn is_read_only_effect(effect: &ToolEffectClass) -> bool {
    matches!(
        effect,
        ToolEffectClass::ReadOnly | ToolEffectClass::ReadValidate | ToolEffectClass::SafeRepeat
    )
}

/// A tool may not be a writer by category while claiming a read-only effect.
///
/// This is the C001 "prospective shared mutator without authority/profile"
/// case: if a writer-category tool declares a read-only effect class, it
/// enters the read-only programme palette while still being able to write.
/// The declaration is not trusted, and the combination is refused.
#[test]
fn writer_category_tools_may_not_declare_a_read_only_effect_class() {
    let (registry, broker) = reviewed_catalog();

    let mut offenders = Vec::new();
    for tool in registry.list() {
        let name = tool.name().to_string();
        if !writer_categories().contains(&tool.category()) {
            continue;
        }
        let Some(contract) = broker.catalog().get(&name) else {
            continue;
        };
        if is_read_only_effect(&contract.effect_class) {
            offenders.push(format!(
                "{name} is {:?} but declares a read-only effect class",
                tool.category()
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "a writer-category tool may not claim to be read-only: {offenders:?}"
    );
}

/// Only reviewed tools may declare a read-only effect class.
///
/// A new tool that self-declares read-only is fail-closed out of the
/// programme palette by default, but it must still be reviewed before it is
/// trusted, so the reviewed set is asserted exactly.
#[test]
fn only_reviewed_tools_declare_a_read_only_effect_class() {
    let (registry, broker) = reviewed_catalog();

    let mut declared = BTreeSet::new();
    for tool in registry.list() {
        let name = tool.name().to_string();
        let Some(contract) = broker.catalog().get(&name) else {
            continue;
        };
        if is_read_only_effect(&contract.effect_class) {
            declared.insert(name);
        }
    }

    let reviewed: BTreeSet<String> = REVIEWED_READ_ONLY_PALETTE
        .iter()
        .map(|s| s.to_string())
        .collect();

    let undeclared: Vec<&String> = declared.difference(&reviewed).collect();
    let stale: Vec<&String> = reviewed.difference(&declared).collect();

    assert!(
        undeclared.is_empty(),
        "these tools declare a read-only effect class but are not in the C001 reviewed \
         palette -- review them before they can be trusted: {undeclared:?}"
    );
    assert!(
        stale.is_empty(),
        "the C001 reviewed read-only palette names tools that no longer declare a \
         read-only effect class: {stale:?}"
    );
}

/// Every writer-category tool must carry a declared disposition, and no
/// writer may be classified as a harmless read.
#[test]
fn every_writer_tool_has_a_disposition_that_is_not_a_read() {
    let registry = ToolRegistry::with_defaults();
    let manifest = load_manifest();

    for tool in registry.list() {
        let name = tool.name().to_string();
        if !writer_categories().contains(&tool.category()) {
            continue;
        }
        let entry = manifest
            .iter()
            .find(|(tool_name, _, _)| *tool_name == name)
            .unwrap_or_else(|| {
                panic!("writer tool '{name}' has no disposition in docs/writer-disposition.toml")
            });
        assert_ne!(
            entry.2, "allowed-read",
            "writer tool '{name}' may not be classified as a harmless read"
        );
        // A writer-category tool may mutate managed state without touching
        // a checkout (`image` performs no filesystem effect at all), so the
        // scope may be `non-checkout`. It may never claim to be read-only,
        // and it is never an external program.
        assert!(
            entry.1 == "checkout-writer" || entry.1 == "non-checkout",
            "writer tool '{name}' must be scoped as a checkout writer or as \
             non-checkout managed state, got '{}'",
            entry.1
        );
    }
}

/// The manifest is a contract, not an authority.
///
/// C001 enables nothing. If a future edit gives the manifest an enabling
/// vocabulary, this trips -- the permit arrived with C002 under an accepted
/// ADR-0042, and the dispositions were not edited to accommodate it.
#[test]
fn the_disposition_manifest_grants_no_authority() {
    let raw = std::fs::read_to_string(manifest_path())
        .expect("docs/writer-disposition.toml must exist for the C001 contract");
    let parsed: toml::Value = raw
        .parse()
        .expect("the disposition manifest must be valid TOML");

    for entry in parsed
        .get("root")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
    {
        let disposition = entry
            .get("disposition")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            !AUTHORITY_VOCABULARY.contains(&disposition),
            "disposition '{disposition}' asserts authority; the C001 manifest records \
             what the restricted profile would do and grants nothing"
        );
        for key in ["enabled", "active", "permitted", "granted"] {
            assert!(
                entry.get(key).is_none(),
                "the C001 manifest must not carry an '{key}' field: it is a contract, \
                 not an enablement"
            );
        }
    }
}

/// The legacy isolated mutator continues.
///
/// C001 changed no runtime behaviour: the default catalog still carries its
/// writers, and the broker still resolves their contracts. A prospective
/// shared mutator has no profile to satisfy yet -- which is exactly why
/// C002 exists, and why nothing here is enabled.
#[test]
fn the_legacy_isolated_mutator_still_dispatches() {
    let registry = ToolRegistry::with_defaults();
    let broker = ToolBroker::new(&registry);

    for name in ["write", "edit", "replace", "apply_patch", "bash", "git"] {
        assert!(
            registry.get(name).is_some(),
            "the legacy mutator '{name}' must still be registered"
        );
        assert!(
            broker.lookup_contract(name).is_ok(),
            "the legacy mutator '{name}' must still have a resolvable contract"
        );
    }

    // C002 shipped the checkout writer permit, and deliberately did **not**
    // put it in the tool catalog: C003 and C004 are what route writers
    // through it. So no catalog entry may claim one yet.
    let permit_bearing = broker
        .catalog()
        .get("write")
        .map(|c| c.name.clone())
        .unwrap_or_default();
    assert!(
        !permit_bearing.contains("permit"),
        "the catalog must not carry a shared-checkout permit before C003 routes writers through it"
    );
}
