//! Which layer won each effective configuration key.
//!
//! The normative contract lives in `architecture/config.md`; this header
//! summarises it. The loader is one pipeline — resolve paths, load each file,
//! merge, migrate, validate. This module *reads* that pipeline rather than
//! reimplementing it: effective values come from the real [`merge_configs`]
//! over the real layer documents, so an answer here is the answer the process
//! actually uses.
//!
//! # The attribution rule
//!
//! A key belongs to the last layer whose document *specifies* it. What
//! "specifies" means depends on the merge shape, and the shape is **declared**
//! in [`STRATEGY_TABLE`] rather than guessed:
//!
//! * `combine_depth == 0` — the whole section is replaced by the last layer
//!   that mentions it, so every leaf under it belongs to that layer
//!   (`provider_connections`, `theme`, the `merge_option!` sections).
//! * `combine_depth == 1` — the section's own fields combine individually
//!   (`server`, `scheduler`); a *nested block* still replaces wholesale, so a
//!   layer that names the enclosing block is a candidate too.
//! * `combine_depth == 2` — a map's entries combine *and* each entry's fields
//!   combine (`provider`), so the same reasoning starts one level lower.
//! * [`AttributionRule::Concatenate`] — every containing layer contributes
//!   (`instructions`); the entry names the last one and lists the rest.
//!
//! The engine never guesses a shape today's merge does not have: if the value
//! at a key last changed in a layer that is not a candidate, or the winning
//! layer's own value differs from the effective one, [`analyze`] returns
//! [`ProvenanceError::Unattributable`] naming the key and both layers.
//!
//! # What the effective value is, and is not
//!
//! The fold is `merge_configs` over prefixes of the parsed layers. `Config::load`
//! then calls `Config::migrate`, which rewrites exactly one key — `version`
//! becomes `CONFIG_VERSION`; `migrate_from_v0` changes nothing else. A dump
//! therefore reports the layer-authored `version`, and a caller comparing it
//! against `Config::load()` must account for that one key.
//!
//! # Credentials
//!
//! A credential prints as `(set)`, or `(unset)` when the block exists but the
//! value does not — never as a value. Classification is fail-closed: the leaf
//! names in [`CREDENTIAL_NAMES`] apply anywhere, and [`CREDENTIAL_CONTAINERS`]
//! redact whole subtrees whose leaf names carry no hint, so an
//! `mcp.*.headers.Authorization` value is a credential under any name.
//! `scripts/check_config_provenance.py` fails when a credential-shaped schema
//! field has no declared owner — a guard failure, not a disclosure.

use std::path::PathBuf;

use serde_json::Value;

use crate::paths::{merge_configs, ConfigLayerKind};
use crate::schema::Config;

/// How `merge_configs` treats a section, as a documentation column.
///
/// The shape and the enforced rule are separate facts on purpose: the shape is
/// what an operator reads, the rule is what the engine checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeShape {
    /// The later layer's value replaces the section whole.
    WholeValue,
    /// `WholeValue`, except the paths in
    /// [`StrategyEntry::inherited_paths`] are carried forward from the layer
    /// beneath when the later layer leaves them unset.
    WholeValueWithInherit,
    /// The section's own fields combine individually; a nested block replaces
    /// wholesale.
    FieldByField,
    /// A map section whose entries combine; keys from earlier layers survive.
    UnionOfKeys,
    /// Every containing layer contributes to one value, in precedence order.
    Concatenate,
}

impl MergeShape {
    /// The stable name used in the generated catalog.
    pub fn as_str(self) -> &'static str {
        match self {
            MergeShape::WholeValue => "whole-value",
            MergeShape::WholeValueWithInherit => "whole-value-with-inherit",
            MergeShape::FieldByField => "field-by-field",
            MergeShape::UnionOfKeys => "union-of-keys",
            MergeShape::Concatenate => "concatenate",
        }
    }

    /// The only shape whose combine depth differs between sections of the same
    /// shape (`provider` is two, every other union is one), so the depth is
    /// declared per entry and this is the value the guard cross-checks.
    pub fn expected_combine_depth(self) -> &'static [u8] {
        match self {
            MergeShape::WholeValue | MergeShape::WholeValueWithInherit => &[0],
            MergeShape::FieldByField => &[1],
            MergeShape::UnionOfKeys => &[1, 2],
            MergeShape::Concatenate => &[0],
        }
    }
}

/// The rule the engine enforces for a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionRule {
    /// The last layer that specifies the key, or the block that contains it,
    /// when the block is replaced wholesale.
    LastContaining,
    /// Every layer that specifies the key contributes, in precedence order.
    Concatenate,
}

impl AttributionRule {
    /// The stable name used in the generated catalog and by the guard.
    pub fn as_str(self) -> &'static str {
        match self {
            AttributionRule::LastContaining => "last-containing",
            AttributionRule::Concatenate => "concatenate",
        }
    }
}

/// One `Config` section's declared merge behaviour.
///
/// Every `Config` field must appear exactly once. The field set is derived from
/// the type by the catalog test's sentinel constructor, and
/// `scripts/check_config_merge_coverage.py` cross-checks this table against the
/// merge source, so a section cannot be added without a declaration and a
/// declaration cannot describe a merge that does not exist.
#[derive(Debug, Clone, Copy)]
pub struct StrategyEntry {
    /// The top-level `Config` field name, i.e. the serialized key.
    pub section: &'static str,
    /// What the merge does, for documentation.
    pub shape: MergeShape,
    /// What the engine enforces.
    pub rule: AttributionRule,
    /// How many leading path segments combine field by field. `0` means the
    /// section itself replaces whole; `1` means its own fields combine; `2`
    /// means a map's entries and their fields combine.
    pub combine_depth: u8,
    /// Paths (relative to the section) that combine at their own level even
    /// though the section replaces whole.
    pub inherited_paths: &'static [&'static str],
    /// Why the shape is not a plain last-wins replacement. Required for every
    /// exception; the guard rejects an exception without one.
    pub note: &'static str,
}

/// The declared strategy for every `Config` section, grouped by merge shape.
///
/// The grouping comments mirror `paths.rs`'s merge arms so the two read
/// together. `scripts/check_config_merge_coverage.py` is what proves the sets
/// are equal.
pub const STRATEGY_TABLE: &[StrategyEntry] = &[
    // `merge_option!` — a plain optional override; the later layer's value
    // replaces, including when the value is a map or a list.
    scalar("schema"),
    scalar("version"),
    scalar("log_level"),
    scalar("model"),
    scalar("small_model"),
    scalar("medium_model"),
    scalar("auto_route_models"),
    scalar("default_agent"),
    scalar("username"),
    scalar("share"),
    scalar("autoupdate"),
    scalar("disabled_providers"),
    scalar("enabled_providers"),
    scalar("permission"),
    scalar("compaction"),
    scalar("subagent"),
    scalar("skills"),
    scalar("templates"),
    scalar("layout"),
    scalar("tools"),
    scalar("formatter"),
    scalar("lsp"),
    scalar("lsp_process_admission"),
    scalar("lsp_semantic_cache"),
    scalar("snapshot"),
    scalar("snapshot_config"),
    scalar("plugin"),
    scalar("enterprise"),
    scalar("experimental"),
    scalar("keybinds"),
    scalar("vim_mode"),
    scalar("panel_hotkeys"),
    scalar("panel_placement"),
    scalar("hooks"),
    scalar("notifications"),
    scalar("catalog"),
    scalar("context"),
    scalar("context_packer"),
    scalar("context_policy"),
    scalar("tool_advisor"),
    scalar("decision_engine"),
    scalar("orchestration"),
    // A map whose entries are replaced per key.
    StrategyEntry {
        section: "model_routers",
        shape: MergeShape::UnionOfKeys,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    // Nested structs that own a `merge` method: their own fields combine.
    StrategyEntry {
        section: "discovery",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "server",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    // A map whose entries own a `merge` method: entries combine and each
    // entry's fields combine, so the combine depth is one level deeper.
    StrategyEntry {
        section: "provider",
        shape: MergeShape::UnionOfKeys,
        rule: AttributionRule::LastContaining,
        combine_depth: 2,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "eggwork",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    // A map of whole entries.
    StrategyEntry {
        section: "agent",
        shape: MergeShape::UnionOfKeys,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "mcp",
        shape: MergeShape::UnionOfKeys,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "model_profile",
        shape: MergeShape::UnionOfKeys,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "commands",
        shape: MergeShape::UnionOfKeys,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "mode",
        shape: MergeShape::UnionOfKeys,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    // Appended, not replaced: the effective list is every layer's entries in
    // precedence order, so no single layer "won" it.
    StrategyEntry {
        section: "instructions",
        shape: MergeShape::Concatenate,
        rule: AttributionRule::Concatenate,
        combine_depth: 0,
        inherited_paths: &[],
        note: "appended to the list in precedence order; every containing layer contributes",
    },
    StrategyEntry {
        section: "theme",
        shape: MergeShape::WholeValue,
        rule: AttributionRule::LastContaining,
        combine_depth: 0,
        inherited_paths: &[],
        note: "",
    },
    // Field-by-field sections added with the merge arms that made them live.
    StrategyEntry {
        section: "search",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "watcher",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "approval_reviewer",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "daemon",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "scratch",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "scheduler",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "tool_deferral",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "research",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "tool_backends",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "human_shell",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "shell",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "preflight",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "command_intent",
        shape: MergeShape::FieldByField,
        rule: AttributionRule::LastContaining,
        combine_depth: 1,
        inherited_paths: &[],
        note: "",
    },
    // Whole-value replace over a struct whose every field is non-`Option`, so
    // serde has already baked the default into the parsed value.
    StrategyEntry {
        section: "provider_connections",
        shape: MergeShape::WholeValue,
        rule: AttributionRule::LastContaining,
        combine_depth: 0,
        inherited_paths: &[],
        note: "",
    },
    StrategyEntry {
        section: "security",
        shape: MergeShape::WholeValueWithInherit,
        rule: AttributionRule::LastContaining,
        combine_depth: 0,
        inherited_paths: &["gates.deny_critical_commands"],
        note: "replaced whole, except `gates.deny_critical_commands`, which is \
               carried forward from the layer beneath when the later layer \
               leaves it unset so the critical-command gate is never lost",
    },
    StrategyEntry {
        section: "deterministic_tools",
        shape: MergeShape::WholeValue,
        rule: AttributionRule::LastContaining,
        combine_depth: 0,
        inherited_paths: &[],
        note: "",
    },
];

/// A plain optional override.
const fn scalar(section: &'static str) -> StrategyEntry {
    StrategyEntry {
        section,
        shape: MergeShape::WholeValue,
        rule: AttributionRule::LastContaining,
        combine_depth: 0,
        inherited_paths: &[],
        note: "",
    }
}

/// The declared strategy for a section, or `None` for a key this table does
/// not know (an unknown key is ignored by the loader too).
///
/// The lookup accepts the serialized spelling: `$schema` is the serde rename of
/// the `schema` field, so a `$` prefix is stripped before the table is
/// consulted. `$`-prefixed keys are serde's convention for meta keys.
pub fn strategy_for(section: &str) -> Option<&'static StrategyEntry> {
    let name = section.strip_prefix('$').unwrap_or(section);
    STRATEGY_TABLE.iter().find(|entry| entry.section == name)
}

/// One layer's parse result.
#[derive(Debug, Clone)]
pub enum LayerContents {
    /// The layer parsed: its typed value and the document it came from.
    Parsed {
        /// The typed layer value, as the merge sees it.
        config: Box<Config>,
        /// The layer's own JSON5 document, which records what the file
        /// actually specified — the fact the typed value cannot carry.
        document: Value,
    },
    /// The layer failed: the path and error are reported, and the other layers
    /// still produce a dump.
    Failed {
        /// The parse or read error, with the path already in its message.
        error: String,
    },
}

/// One configuration layer: where it came from and what it said.
#[derive(Debug, Clone)]
pub struct Layer {
    /// The layer's precedence identity.
    pub kind: ConfigLayerKind,
    /// The file it was read from.
    pub path: PathBuf,
    /// Its parse result.
    pub contents: LayerContents,
}

impl Layer {
    /// The typed value, when the layer parsed.
    fn config(&self) -> Option<&Config> {
        match &self.contents {
            LayerContents::Parsed { config, .. } => Some(config),
            LayerContents::Failed { .. } => None,
        }
    }

    /// The raw document, when the layer parsed.
    fn document(&self) -> Option<&Value> {
        match &self.contents {
            LayerContents::Parsed { document, .. } => Some(document),
            LayerContents::Failed { .. } => None,
        }
    }
}

/// Read every discovered layer from disk.
///
/// A layer that fails to read or parse is kept as [`LayerContents::Failed`];
/// "the project layer is broken" is the most useful answer a provenance dump
/// can give, so it is a result, not a reason to stop.
pub fn load_layers() -> Vec<Layer> {
    crate::paths::resolve_config_layer_paths()
        .into_iter()
        .map(|(kind, path)| {
            let contents = match crate::paths::load_config_document(&path) {
                Ok((config, document)) => LayerContents::Parsed {
                    config: Box::new(config),
                    document,
                },
                Err(error) => LayerContents::Failed {
                    error: error.to_string(),
                },
            };
            Layer {
                kind,
                path,
                contents,
            }
        })
        .collect()
}

/// Parse in-memory layer sources, in the order given.
///
/// The provenance engine and its tests share this constructor, so a fixture
/// goes through the same parse a file does.
pub fn layers_from_sources(sources: &[(ConfigLayerKind, PathBuf, &str)]) -> Vec<Layer> {
    sources
        .iter()
        .map(|(kind, path, source)| {
            let contents = match crate::paths::parse_config_document(source, path) {
                Ok((config, document)) => LayerContents::Parsed {
                    config: Box::new(config),
                    document,
                },
                Err(error) => LayerContents::Failed {
                    error: error.to_string(),
                },
            };
            Layer {
                kind: *kind,
                path: path.clone(),
                contents,
            }
        })
        .collect()
}

/// One value a layer specified for a key that a later layer overrode, or (for
/// a concatenating section) contributed alongside the winner.
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowedValue {
    /// The layer that specified it.
    pub layer: ConfigLayerKind,
    /// Its rendered value, already redacted.
    pub value: String,
}

/// One effective key.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The dotted key path, using the document's own spelling.
    pub path: String,
    /// The rendered value, already redacted.
    pub value: String,
    /// The layer that won the key, or `None` when no layer specified it (the
    /// table's defaults did).
    pub layer: Option<ConfigLayerKind>,
    /// Earlier layers' values for the same key. For a concatenating section
    /// these are contributors, not overridden values.
    pub shadowed: Vec<ShadowedValue>,
    /// Whether the value was redacted.
    pub credential: bool,
    /// The rule that attributed it.
    pub rule: AttributionRule,
}

/// One layer's status, for the dump's header.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerStatus {
    /// Its precedence identity.
    pub kind: ConfigLayerKind,
    /// The path it was read from.
    pub path: PathBuf,
    /// The parse error, when it did not parse.
    pub error: Option<String>,
}

/// The dump.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// Every discovered layer, in precedence order, including broken ones.
    pub layers: Vec<LayerStatus>,
    /// Every effective key, deterministically ordered by path.
    pub entries: Vec<Entry>,
}

/// Why a key could not be attributed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvenanceError {
    /// The merge is not last-wins for this key and the section does not say so.
    Unattributable {
        /// The key path.
        path: String,
        /// The layer the declared rule made the candidate, if any.
        attributed: Option<ConfigLayerKind>,
        /// The layer in which the serialized value last changed.
        changed_in: Option<ConfigLayerKind>,
        /// What disagreed: the value, or the fold order.
        detail: String,
    },
}

impl std::fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProvenanceError::Unattributable {
                path,
                attributed,
                changed_in,
                detail,
            } => {
                let attributed = attributed
                    .map(|layer| layer.to_string())
                    .unwrap_or_else(|| "no layer".to_string());
                let changed_in = changed_in
                    .map(|layer| layer.to_string())
                    .unwrap_or_else(|| "no layer".to_string());
                write!(
                    f,
                    "config key `{path}` is not attributable to a layer: \
                     the merge rule names {attributed}, but {detail} ({changed_in}). \
                     The section's merge is not last-wins and must be declared in \
                     STRATEGY_TABLE with its own attribution rule."
                )
            }
        }
    }
}

impl std::error::Error for ProvenanceError {}

/// Attribute every effective key in `layers` to the layer that won it.
///
/// The fold is [`merge_configs`] over prefixes of the parsed layers, so the
/// serialized value after each fold is what the real pipeline produced.
pub fn analyze(layers: &[Layer]) -> Result<Report, ProvenanceError> {
    analyze_with(layers, None)
}

/// [`analyze`], with an optional override for the attribution rule of every
/// section.
///
/// The override exists so a test can remove one declaration and prove the
/// engine rejects the resulting mis-attribution instead of printing a
/// confident wrong answer. Production passes `None`: the table is the only
/// rule source.
pub fn analyze_with(
    layers: &[Layer],
    rule_override: Option<&dyn Fn(&str) -> AttributionRule>,
) -> Result<Report, ProvenanceError> {
    let statuses: Vec<LayerStatus> = layers
        .iter()
        .map(|layer| LayerStatus {
            kind: layer.kind,
            path: layer.path.clone(),
            error: match &layer.contents {
                LayerContents::Parsed { .. } => None,
                LayerContents::Failed { error } => Some(error.clone()),
            },
        })
        .collect();

    // The parsed layers, in precedence order. A broken layer contributes
    // nothing to the fold but stays in the status list.
    let parsed: Vec<&Layer> = layers
        .iter()
        .filter(|layer| layer.config().is_some())
        .collect();

    let configs: Vec<Config> = parsed
        .iter()
        .map(|layer| layer.config().expect("filtered").clone())
        .collect();

    // `folds[i]` is `merge_configs` over layers `0..=i`, serialized. This is
    // the real merge applied to prefixes, not a reimplementation.
    let mut folds: Vec<Value> = Vec::with_capacity(configs.len());
    for end in 1..=configs.len() {
        let merged = merge_configs(&configs[..end]);
        let value =
            serde_json::to_value(&merged).map_err(|error| ProvenanceError::Unattributable {
                path: "<config>".to_string(),
                attributed: None,
                changed_in: None,
                detail: format!("the merged config does not serialize: {error}"),
            })?;
        folds.push(value);
    }
    let effective = folds
        .last()
        .cloned()
        .unwrap_or_else(|| serde_json::to_value(Config::default()).unwrap_or(Value::Null));

    let mut leaves: Vec<(Vec<String>, String, Value)> = Vec::new();
    flatten(&[], &effective, &mut leaves);
    leaves.sort_by(|left, right| left.1.cmp(&right.1));

    let mut entries = Vec::with_capacity(leaves.len());
    for (segments, path, value) in leaves {
        let credential = is_credential_path(&segments);
        // A `null` leaf means "not specified anywhere". Credential leaves are
        // kept as `(unset)` so the dump can distinguish "no credential" from
        // "no such provider"; every other unset key is simply absent.
        if value.is_null() && !credential {
            continue;
        }
        let section = segments.first().map(String::as_str).unwrap_or_default();
        let declared = strategy_for(section);
        let rule = match rule_override {
            Some(override_fn) => override_fn(section),
            None => declared
                .map(|entry| entry.rule)
                .unwrap_or(AttributionRule::LastContaining),
        };

        let entry = match rule {
            AttributionRule::Concatenate => {
                attribute_concatenation(&parsed, &segments, &path, &value, credential)
            }
            AttributionRule::LastContaining => attribute_last_containing(
                &parsed, &folds, declared, &segments, &path, &value, credential,
            )?,
        };
        entries.push(entry);
    }

    Ok(Report {
        layers: statuses,
        entries,
    })
}

/// Attribute a leaf of a last-wins section, and prove the answer.
fn attribute_last_containing(
    layers: &[&Layer],
    folds: &[Value],
    declared: Option<&StrategyEntry>,
    segments: &[String],
    path: &str,
    value: &Value,
    credential: bool,
) -> Result<Entry, ProvenanceError> {
    let combine_depth = usize::from(declared.map(|entry| entry.combine_depth).unwrap_or(0));
    let inherited = is_inherited_path(declared, segments);
    // The candidate prefixes: the key itself, plus — when the merge replaces a
    // containing block wholesale — the block paths the later layers can have
    // named. An inherited path is never replaced, so it has no candidates but
    // itself.
    let candidate_lengths = candidate_prefix_lengths(segments.len(), combine_depth, inherited);

    let mut attributed: Option<(usize, &Layer)> = None;
    for (index, layer) in layers.iter().enumerate().rev() {
        let Some(document) = layer.document() else {
            continue;
        };
        let named = candidate_lengths.iter().any(|length| {
            lookup(document, &segments[..*length])
                .map(|found| !found.is_null())
                .unwrap_or(false)
        });
        if named {
            attributed = Some((index, layer));
            break;
        }
    }

    let changed_in = last_changed_fold(folds, segments);
    let attributed_layer = attributed.map(|(_, layer)| layer.kind);
    let changed_layer = changed_in.and_then(|index| layers.get(index - 1).map(|l| l.kind));

    // The serialized value must not change in a layer that is not a candidate
    // for this key: that is a merge which is not the declared rule.
    if let Some(changed) = changed_in {
        let candidate = attributed.map(|(index, _)| index + 1).unwrap_or(0);
        if changed > candidate {
            return Err(ProvenanceError::Unattributable {
                path: path.to_string(),
                attributed: attributed_layer,
                changed_in: changed_layer,
                detail: "the effective value changed in a layer that does not specify the \
                         key or the block containing it"
                    .to_string(),
            });
        }
    }

    // The winning layer's own value must equal the effective value. When the
    // key was attributed through an enclosing block the layer need not name
    // the key at all, so there is nothing to compare against.
    if let Some((index, layer)) = attributed {
        if let Some(document) = layer.document() {
            if let Some(own) = lookup(document, segments).filter(|found| !found.is_null()) {
                if !values_equal(own, value) {
                    return Err(ProvenanceError::Unattributable {
                        path: path.to_string(),
                        attributed: attributed_layer,
                        changed_in: changed_layer,
                        detail: "that layer's own value differs from the effective one".to_string(),
                    });
                }
            }
        }
        let shadowed = shadowed_values(layers, segments, index, credential);
        return Ok(Entry {
            path: path.to_string(),
            value: display_value(value, credential),
            layer: Some(layer.kind),
            shadowed,
            credential,
            rule: AttributionRule::LastContaining,
        });
    }

    Ok(Entry {
        path: path.to_string(),
        value: display_value(value, credential),
        layer: None,
        shadowed: Vec::new(),
        credential,
        rule: AttributionRule::LastContaining,
    })
}

/// Attribute a leaf of an appending section: every containing layer is a
/// contributor, and the last one is named as the winner.
fn attribute_concatenation(
    layers: &[&Layer],
    segments: &[String],
    path: &str,
    value: &Value,
    credential: bool,
) -> Entry {
    let contributors: Vec<(usize, &Layer)> = layers
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, layer)| {
            layer
                .document()
                .and_then(|document| lookup(document, segments))
                .map(|found| !found.is_null())
                .unwrap_or(false)
        })
        .collect();
    let layer = contributors.last().map(|(_, layer)| layer.kind);
    let shadowed = contributors
        .iter()
        .take(contributors.len().saturating_sub(1))
        .filter_map(|(_, layer)| {
            layer.document().and_then(|document| {
                lookup(document, segments).map(|own| ShadowedValue {
                    layer: layer.kind,
                    value: display_value(own, credential),
                })
            })
        })
        .collect();
    Entry {
        path: path.to_string(),
        value: display_value(value, credential),
        layer,
        shadowed,
        credential,
        rule: AttributionRule::Concatenate,
    }
}

/// Which prefixes of a path a layer may have named and still win the leaf.
fn candidate_prefix_lengths(segments: usize, combine_depth: usize, inherited: bool) -> Vec<usize> {
    let mut lengths = vec![segments];
    if !inherited {
        // Ancestors at or below the deepest combining level are replaced
        // wholesale by a layer that names them.
        for length in (combine_depth + 1).max(1)..segments {
            lengths.push(length);
        }
    }
    lengths
}

/// Whether the section declares this path as carried forward rather than
/// replaced, e.g. `security.gates.deny_critical_commands`.
fn is_inherited_path(declared: Option<&StrategyEntry>, segments: &[String]) -> bool {
    let Some(entry) = declared else {
        return false;
    };
    if entry.inherited_paths.is_empty() || segments.is_empty() {
        return false;
    }
    let relative = segments[1..].join(".");
    entry.inherited_paths.contains(&relative.as_str())
}

/// The 1-based fold in which the value at `segments` last changed, or `None`.
fn last_changed_fold(folds: &[Value], segments: &[String]) -> Option<usize> {
    let mut previous: Option<&Value> = None;
    let mut last = None;
    for (index, fold) in folds.iter().enumerate() {
        let current = lookup(fold, segments).filter(|found| !found.is_null());
        if current != previous {
            last = Some(index + 1);
        }
        previous = current;
    }
    last
}

/// Every earlier layer that named the key, with its rendered value.
fn shadowed_values(
    layers: &[&Layer],
    segments: &[String],
    winner: usize,
    credential: bool,
) -> Vec<ShadowedValue> {
    let mut shadowed = Vec::new();
    for layer in layers.iter().take(winner) {
        let Some(document) = layer.document() else {
            continue;
        };
        let Some(own) = lookup(document, segments).filter(|found| !found.is_null()) else {
            continue;
        };
        shadowed.push(ShadowedValue {
            layer: layer.kind,
            value: display_value(own, credential),
        });
    }
    shadowed
}

/// Flatten an object tree to leaves. Arrays are leaves: an index is not a
/// configuration key, and attributing one would suggest it is.
fn flatten(prefix: &[String], value: &Value, out: &mut Vec<(Vec<String>, String, Value)>) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                let mut next = prefix.to_vec();
                next.push(key.clone());
                flatten(&next, &map[key], out);
            }
        }
        other => {
            let path = prefix.join(".");
            out.push((prefix.to_vec(), path, other.clone()));
        }
    }
}

/// Walk a document by path. A missing object ends the walk.
fn lookup<'a>(document: &'a Value, segments: &[String]) -> Option<&'a Value> {
    let mut current = document;
    for segment in segments {
        current = current.as_object()?.get(segment)?;
    }
    Some(current)
}

/// Values compare by their serialized form, so `1` and `1.0` are distinct the
/// way a config file sees them.
fn values_equal(left: &Value, right: &Value) -> bool {
    left == right
}

/// Render one leaf. A credential never renders its value.
fn display_value(value: &Value, credential: bool) -> String {
    if credential {
        return if value.is_null() {
            "(unset)".to_string()
        } else {
            "(set)".to_string()
        };
    }
    match value {
        Value::Null => "(unset)".to_string(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Leaf names that are credentials wherever they appear.
///
/// Fail-closed: a name in this set is redacted even if the schema did not
/// anticipate it.
pub const CREDENTIAL_NAMES: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "encrypted_api_key",
    "auth_token",
    "access_token",
    "refresh_token",
    "id_token",
    "bearer",
    "client_secret",
    "clientsecret",
    "private_key",
    "privatekey",
    "password",
    "passwd",
    "secret",
    "token",
    "credential",
    "credentials",
];

/// Path segments whose entire subtree is a credential, whatever the leaf
/// names.
///
/// These blocks hold operator-chosen keys (`env`, `headers`), an auth
/// descriptor (`auth`, `oauth`) or their documented equivalents, so no leaf
/// name carries a signal. The rule is global — not scoped to one section —
/// because a block named `env` is an environment map wherever it appears, and a
/// redaction that misses one of them is a disclosure.
pub const CREDENTIAL_CONTAINERS: &[&str] = &["auth", "env", "environment", "headers", "oauth"];

/// Every schema field that holds a credential, with the rule that redacts it.
///
/// `scripts/check_config_provenance.py` requires this list to equal the set of
/// fields in `schema.rs` whose name is credential-shaped. A credential field
/// with no entry is a disclosure the schema never marked, so it is a guard
/// failure rather than a name heuristic's call. Causes:
///
/// * `name` — the leaf name is in [`CREDENTIAL_NAMES`].
/// * `container:<segment>` — the field is a block in [`CREDENTIAL_CONTAINERS`].
/// * `open-map` — an open map of provider- or agent-specific knobs. Its entries
///   are redacted by their own key name, which is the only signal such a map
///   has; a secret stored under an innocuous key is the residual risk this
///   cause names rather than hides.
/// * `not-a-credential` — the name matches the detector but the field holds no
///   secret.
pub const CREDENTIAL_FIELDS: &[(&str, &str, &str)] = &[
    ("AgentConfig", "options", "open-map"),
    ("CommandConfig", "env", "container:env"),
    ("DecisionEngineConfig", "auth", "container:auth"),
    ("EggsearchConfig", "env", "container:env"),
    ("ExternalToolBackendConfigSchema", "env", "container:env"),
    ("FormatterRule", "environment", "container:environment"),
    ("McpOAuthConfig", "client_secret", "name"),
    ("McpServerConfig", "env", "container:env"),
    ("McpServerConfig", "environment", "container:environment"),
    ("McpServerConfig", "headers", "container:headers"),
    ("McpServerConfig", "oauth", "container:oauth"),
    ("ProviderConfig", "api_key", "name"),
    ("ProviderConfig", "auth", "container:auth"),
    ("ProviderConfig", "encrypted_api_key", "name"),
    ("ProviderConfig", "options", "open-map"),
    ("SearchProviderConfig", "api_key", "name"),
    (
        "SecurityGateConfig",
        "ask_on_secret_exposure",
        "not-a-credential",
    ),
    ("ServerConfig", "token", "name"),
];

/// Whether this leaf is a credential and must not be printed.
pub fn is_credential_path(segments: &[String]) -> bool {
    let Some(last) = segments.last() else {
        return false;
    };
    if CREDENTIAL_NAMES.contains(&last.to_ascii_lowercase().as_str()) {
        return true;
    }
    segments.iter().any(|segment| {
        CREDENTIAL_CONTAINERS
            .iter()
            .any(|container| segment.eq_ignore_ascii_case(container))
    })
}

/// Render the dump for a terminal.
///
/// Layers first — including broken ones — then keys grouped by winning layer,
/// each with the values it overrode beneath it.
pub fn render_text(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("layers (lowest precedence first):\n");
    if report.layers.is_empty() {
        out.push_str("  (no configuration file was found)\n");
    }
    for layer in &report.layers {
        match &layer.error {
            Some(error) => {
                out.push_str(&format!(
                    "  [error] {}  {}\n",
                    layer.kind,
                    layer.path.display()
                ));
                out.push_str(&format!("          {error}\n"));
            }
            None => out.push_str(&format!(
                "  [ok]    {}  {}\n",
                layer.kind,
                layer.path.display()
            )),
        }
    }

    if report.entries.is_empty() {
        out.push_str("effective keys: (none; every key is at its default)\n");
        return out;
    }

    let mut groups: Vec<(Option<ConfigLayerKind>, Vec<&Entry>)> = Vec::new();
    for kind in ConfigLayerKind::ALL {
        let entries: Vec<&Entry> = report
            .entries
            .iter()
            .filter(|entry| entry.layer == Some(kind))
            .collect();
        if !entries.is_empty() {
            groups.push((Some(kind), entries));
        }
    }
    let defaults: Vec<&Entry> = report
        .entries
        .iter()
        .filter(|entry| entry.layer.is_none())
        .collect();
    if !defaults.is_empty() {
        groups.push((None, defaults));
    }

    out.push_str("effective keys (grouped by winning layer):\n");
    for (kind, entries) in groups {
        let heading = kind
            .map(|kind| kind.to_string())
            .unwrap_or_else(|| "(defaults)".to_string());
        out.push_str(&format!("== {heading} ==\n"));
        for entry in entries {
            let suffix = match entry.rule {
                AttributionRule::Concatenate => "  [every containing layer contributes]",
                AttributionRule::LastContaining => "",
            };
            out.push_str(&format!("  {} = {}{suffix}\n", entry.path, entry.value));
            for shadowed in &entry.shadowed {
                out.push_str(&format!(
                    "      shadowed: {} = {}\n",
                    shadowed.layer, shadowed.value
                ));
            }
        }
    }
    out
}

/// Render the dump as JSON: an array of
/// `{ path, value, layer, shadowed: [{ layer, value }] }`, ordered by path.
pub fn render_json(report: &Report) -> String {
    let entries: Vec<Value> = report
        .entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "path": entry.path,
                "value": entry.value,
                "layer": entry.layer.map(|layer| layer.to_string()).unwrap_or_else(|| "default".to_string()),
                "shadowed": entry
                    .shadowed
                    .iter()
                    .map(|shadowed| serde_json::json!({
                        "layer": shadowed.layer.to_string(),
                        "value": shadowed.value,
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::to_string(&sorted_json(Value::Array(entries))).unwrap_or_else(|_| "[]".to_string())
}

/// Rebuild every object in `value` with its keys in sorted order.
///
/// `serde_json::Map` is a `BTreeMap` — sorted — **unless** another crate in the
/// build graph enables `serde_json/preserve_order`, in which case it is an
/// insertion-ordered map. That made the emitted key order a property of the
/// build rather than of this function: `cargo test -p codegg-config` produced
/// `layer, path, shadowed, value` while `cargo nextest run --workspace`
/// produced the order `json!` inserted them in, so a captured fixture could
/// match one and never the other. A renderer whose output depends on which
/// other crates happened to be compiled is not reproducible — and the
/// redaction fixture is a byte golden precisely so its output can be diffed.
///
/// Sorting here is idempotent under both configurations: with `preserve_order`
/// off the map is already ordered, and with it on these inserts are the order.
fn sorted_json(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut ordered = std::collections::BTreeMap::new();
            for (key, inner) in map {
                ordered.insert(key, sorted_json(inner));
            }
            let mut out = serde_json::Map::new();
            for (key, inner) in ordered {
                out.insert(key, inner);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted_json).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build layers from in-memory documents, in the order given.
    fn layers(sources: &[(ConfigLayerKind, &str)]) -> Vec<Layer> {
        let inputs: Vec<(ConfigLayerKind, PathBuf, &str)> = sources
            .iter()
            .map(|(kind, source)| {
                (
                    *kind,
                    PathBuf::from(format!("/fixture/{kind}.json")),
                    *source,
                )
            })
            .collect();
        layers_from_sources(&inputs)
    }

    /// The entry for `path`, or a panic listing what the report has.
    fn entry<'a>(report: &'a Report, path: &str) -> &'a Entry {
        report
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap_or_else(|| {
                panic!(
                    "no entry `{path}`; report has {:?}",
                    report
                        .entries
                        .iter()
                        .map(|entry| entry.path.as_str())
                        .collect::<Vec<_>>()
                )
            })
    }

    #[test]
    fn field_by_field_section_attributes_each_key_to_its_own_layer() {
        let report = analyze(&layers(&[
            (ConfigLayerKind::System, r#"{"server": {"mdns": true}}"#),
            (
                ConfigLayerKind::Global,
                r#"{"server": {"hostname": "global-host"}}"#,
            ),
            (ConfigLayerKind::Project, r#"{"server": {"port": 8080}}"#),
        ]))
        .expect("a last-wins section is attributable");
        assert_eq!(
            entry(&report, "server.mdns").layer,
            Some(ConfigLayerKind::System)
        );
        assert_eq!(
            entry(&report, "server.hostname").layer,
            Some(ConfigLayerKind::Global)
        );
        assert_eq!(
            entry(&report, "server.port").layer,
            Some(ConfigLayerKind::Project)
        );
        assert_eq!(entry(&report, "server.port").value, "8080");
    }

    #[test]
    fn whole_value_section_attributes_every_key_to_the_later_layer() {
        let report = analyze(&layers(&[
            (
                ConfigLayerKind::System,
                r#"{"provider_connections": {"background_refresh": true, "global_refresh_cap": 2}}"#,
            ),
            (
                ConfigLayerKind::Project,
                r#"{"provider_connections": {"max_concurrent_refreshes": 4}}"#,
            ),
        ]))
        .expect("a whole-value section is attributable");
        let under: Vec<&Entry> = report
            .entries
            .iter()
            .filter(|entry| entry.path.starts_with("provider_connections."))
            .collect();
        assert!(
            under.len() >= 3,
            "the section's whole field set is reported, got {under:?}"
        );
        for entry in under {
            assert_eq!(
                entry.layer,
                Some(ConfigLayerKind::Project),
                "`{}` must belong to the layer that replaced the section",
                entry.path
            );
        }
    }

    #[test]
    fn concatenated_section_names_every_contributing_layer() {
        let report = analyze(&layers(&[
            (ConfigLayerKind::System, r#"{"instructions": ["a"]}"#),
            (ConfigLayerKind::Global, r#"{"instructions": ["b"]}"#),
            (ConfigLayerKind::Project, r#"{"instructions": ["c"]}"#),
        ]))
        .expect("a declared concatenation is attributable");
        let instructions = entry(&report, "instructions");
        assert_eq!(instructions.value, r#"["a","b","c"]"#);
        assert_eq!(instructions.layer, Some(ConfigLayerKind::Project));
        assert_eq!(instructions.rule, AttributionRule::Concatenate);
        let contributors: Vec<ConfigLayerKind> =
            instructions.shadowed.iter().map(|s| s.layer).collect();
        assert_eq!(
            contributors,
            vec![ConfigLayerKind::System, ConfigLayerKind::Global]
        );
    }

    #[test]
    fn undeclared_non_last_wins_merge_is_reported_not_guessed() {
        let layers = layers(&[
            (ConfigLayerKind::System, r#"{"instructions": ["a"]}"#),
            (ConfigLayerKind::Project, r#"{"instructions": ["b"]}"#),
        ]);
        let error = analyze_with(&layers, Some(&|_section| AttributionRule::LastContaining))
            .expect_err("append-only merge reported as last-wins must be refused");
        match error {
            ProvenanceError::Unattributable {
                path, attributed, ..
            } => {
                assert_eq!(path, "instructions");
                assert_eq!(attributed, Some(ConfigLayerKind::Project));
            }
        }
    }

    #[test]
    fn declared_inherited_path_is_attributed_to_the_layer_that_set_it() {
        let report = analyze(&layers(&[
            (
                ConfigLayerKind::Global,
                r#"{"security": {"gates": {"deny_critical_commands": false}}}"#,
            ),
            (
                ConfigLayerKind::Project,
                r#"{"security": {"enabled": true}}"#,
            ),
        ]))
        .expect("the declared inherit rule is attributable");
        assert_eq!(
            entry(&report, "security.gates.deny_critical_commands").layer,
            Some(ConfigLayerKind::Global)
        );
        assert_eq!(
            entry(&report, "security.enabled").layer,
            Some(ConfigLayerKind::Project)
        );
    }

    #[test]
    fn report_values_equal_the_merge_of_the_same_documents() {
        let sources = [
            (
                ConfigLayerKind::System,
                r#"{"server": {"port": 8080}, "model": "system/model"}"#,
            ),
            (
                ConfigLayerKind::Project,
                r#"{"model": "project/model", "instructions": ["x"]}"#,
            ),
        ];
        let report = analyze(&layers(&sources)).expect("attributable");
        let configs: Vec<Config> = sources
            .iter()
            .map(|(_, source)| {
                crate::paths::parse_config(source, std::path::Path::new("/fixture/x.json"))
                    .expect("fixture parses")
            })
            .collect();
        let merged = serde_json::to_value(merge_configs(&configs)).expect("serializes");
        let mut direct = Vec::new();
        direct_leaves(&[], &merged, &mut direct);
        for (path, value) in direct {
            let entry = entry(&report, &path);
            assert_eq!(entry.value, display_value(&value, false), "`{path}` value");
        }
    }

    /// A naive leaf walk, independent of the engine's, for the equality test.
    fn direct_leaves(prefix: &[String], value: &Value, out: &mut Vec<(String, Value)>) {
        if let Value::Object(map) = value {
            for (key, child) in map {
                let mut next = prefix.to_vec();
                next.push(key.clone());
                direct_leaves(&next, child, out);
            }
        } else if !value.is_null() {
            out.push((prefix.join("."), value.clone()));
        }
    }

    /// The fixture document the redaction tests share: a secret at every
    /// credential shape the schema has, plus a shadowed one.
    fn secret_report() -> Report {
        analyze(&layers(&[
            (
                ConfigLayerKind::Global,
                r#"{"provider": {"openai": {"api_key": "provider-secret-2"}}}"#,
            ),
            (
                ConfigLayerKind::Project,
                r#"{
                    "server": {"token": "server-secret-1"},
                    "provider": {"openai": {
                        "api_key": "provider-secret-2b",
                        "encrypted_api_key": "provider-secret-3",
                        "auth": {"type": "api_key", "value": "provider-secret-4"}
                    }},
                    "mcp": {"remote": {
                        "env": {"API_TOKEN": "env-secret-5"},
                        "headers": {"Authorization": "header-secret-6"}
                    }}
                }"#,
            ),
        ]))
        .expect("attributable")
    }

    #[test]
    fn credential_values_never_appear_in_either_format() {
        let report = secret_report();
        let text = render_text(&report);
        let json = render_json(&report);
        for secret in [
            "server-secret-1",
            "provider-secret-2",
            "provider-secret-2b",
            "provider-secret-3",
            "provider-secret-4",
            "env-secret-5",
            "header-secret-6",
        ] {
            assert!(!text.contains(secret), "text leaked {secret}: {text}");
            assert!(!json.contains(secret), "json leaked {secret}: {json}");
        }
        assert!(text.contains("server.token = (set)"), "{text}");
        assert!(json.contains(r#""value":"(set)""#), "{json}");
    }

    #[test]
    fn credential_classification_redacts_containers_not_names() {
        let path = |text: &str| -> Vec<String> {
            text.split('.')
                .map(std::string::ToString::to_string)
                .collect()
        };
        assert!(is_credential_path(&path("server.token")));
        assert!(is_credential_path(&path("provider.openai.api_key")));
        assert!(is_credential_path(&path("provider.openai.auth.value")));
        assert!(is_credential_path(&path("mcp.remote.env.PATH")));
        assert!(is_credential_path(&path(
            "mcp.remote.headers.Authorization"
        )));
        assert!(is_credential_path(&path("provider.openai.options.api_key")));
        assert!(!is_credential_path(&path(
            "provider.openai.options.temperature"
        )));
        assert!(!is_credential_path(&path("server.port")));
        assert!(!is_credential_path(&path(
            "scheduler.quota.max_concurrent_builds"
        )));
    }

    #[test]
    fn a_broken_layer_is_reported_and_the_others_still_dump() {
        let report = analyze(&layers(&[
            (ConfigLayerKind::System, r#"{"server": {"port": 8080}}"#),
            (ConfigLayerKind::Project, r#"{"server": {"port": "#),
        ]))
        .expect("a broken layer does not stop attribution");
        assert_eq!(report.layers.len(), 2);
        assert!(report.layers[0].error.is_none());
        assert!(report.layers[1].error.is_some());
        assert_eq!(
            entry(&report, "server.port").layer,
            Some(ConfigLayerKind::System)
        );
        let text = render_text(&report);
        assert!(text.contains("[error] project"), "{text}");
        assert!(text.contains("server.port = 8080"), "{text}");
    }

    #[test]
    fn no_layers_reports_defaults_rather_than_an_empty_document() {
        let report = analyze(&[]).expect("nothing to attribute");
        assert!(report.layers.is_empty());
        assert!(report.entries.is_empty());
        let text = render_text(&report);
        assert!(text.contains("no configuration file was found"), "{text}");
        assert!(text.contains("every key is at its default"), "{text}");
        assert_eq!(render_json(&report), "[]");
    }

    /// The regeneration command for the captured dump, quoted in its failure.
    const UPDATE_FIXTURE_COMMAND: &str =
        "CODEGG_UPDATE_PROVENANCE_FIXTURE=1 cargo test -p codegg-config provenance";

    /// Where a captured dump lives.
    fn fixture_path(extension: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(format!("provenance-redaction.{extension}"))
    }

    #[test]
    fn captured_redaction_fixture_matches_the_renderer() {
        let report = secret_report();
        let capture = [
            ("txt", render_text(&report)),
            ("json", render_json(&report)),
        ];
        let update = std::env::var("CODEGG_UPDATE_PROVENANCE_FIXTURE").as_deref() == Ok("1");
        for (extension, rendered) in capture {
            let path = fixture_path(extension);
            if update {
                std::fs::create_dir_all(path.parent().expect("the fixture has a directory"))
                    .expect("the fixture directory is writable");
                std::fs::write(&path, &rendered).expect("the fixture is writable");
            }
            let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "cannot read {} ({error}); regenerate with `{UPDATE_FIXTURE_COMMAND}`",
                    path.display()
                )
            });
            assert_eq!(
                committed,
                rendered,
                "{} is stale; regenerate with `{UPDATE_FIXTURE_COMMAND}`",
                path.display()
            );
        }
    }

    #[test]
    fn layer_paths_and_layer_kinds_share_one_discovery_order() {
        let discovered = crate::paths::resolve_config_layer_paths();
        let paths: Vec<PathBuf> = discovered
            .iter()
            .map(|(_kind, path)| path.clone())
            .collect();
        assert_eq!(paths, crate::paths::resolve_config_paths());
        let mut last = None;
        for (kind, _path) in &discovered {
            if let Some(previous) = last {
                assert!(
                    previous < *kind,
                    "layer order must be strictly increasing: {previous:?} then {kind:?}"
                );
            }
            last = Some(*kind);
        }
    }
}
