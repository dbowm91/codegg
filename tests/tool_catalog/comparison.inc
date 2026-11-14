//! The authoritative freshness gate for `architecture/tool-catalog.md`.
//!
//! `render.inc` emits the catalog; this member compares it to the committed
//! file. Without `CODEGG_UPDATE_TOOL_CATALOG=1` a mismatch fails and prints the
//! first divergent line plus the update command. With it, the file is rewritten
//! — except under CI, where rewriting the artifact being verified would make the
//! gate prove nothing.
//!
//! The structural guard (`scripts/check_tool_catalog.py`) runs where tests do
//! not; this member is the only place the registry is compared, so it is the
//! only place a stale catalog can be caught before a reader sees it.

use std::fs;

use super::render::{
    catalog_path, catalog_tool_names, first_divergent_line, render_catalog, surface_document_path,
    update_requested, GENERATED_HEADER, UPDATE_COMMAND,
};

#[test]
fn catalog_matches_the_committed_file() {
    let rendered = render_catalog();
    let path = catalog_path();
    let update = update_requested(
        std::env::var("CODEGG_UPDATE_TOOL_CATALOG").ok().as_deref(),
        std::env::var("CI").ok().as_deref(),
    )
    .unwrap_or_else(|refusal| panic!("{refusal}"));
    if update {
        fs::write(&path, &rendered).expect("the catalog is writable");
    }
    let committed = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}); regenerate with `{UPDATE_COMMAND}`",
            path.display()
        )
    });
    if committed != rendered {
        panic!(
            "{} is stale: {}\nregenerate with `{UPDATE_COMMAND}`",
            path.display(),
            first_divergent_line(&committed, &rendered)
        );
    }
}

#[test]
fn the_committed_header_names_its_generator_and_its_update_command() {
    let text = fs::read_to_string(catalog_path()).unwrap_or_else(|error| {
        panic!("cannot read the committed catalog ({error}); generate it with `{UPDATE_COMMAND}`")
    });
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(GENERATED_HEADER),
        "the catalog's first line must be the generator's marker, character for \
         character; a hand-edited header is how a hand-edited file hides"
    );
    assert!(
        lines.next().unwrap_or_default().contains(UPDATE_COMMAND),
        "the catalog's second line must name the command that regenerates it"
    );
}

#[test]
fn the_hand_written_surface_document_carries_no_tool_table() {
    let document = fs::read_to_string(surface_document_path())
        .expect("architecture/agent-tool-surface.md is readable");
    let enumerated: Vec<String> = catalog_tool_names()
        .into_iter()
        .filter(|name| {
            let row = format!("| `{name}`");
            document
                .lines()
                .any(|line| line.trim_start().starts_with(&row))
        })
        .collect();
    assert!(
        enumerated.is_empty(),
        "architecture/agent-tool-surface.md must keep the prose and lose the \
         enumeration — the catalog is the enumeration — but it still tabulates {enumerated:?}"
    );
}
