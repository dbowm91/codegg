//! Per-entry icons for the workspace navigators (the Ctrl+T tree pane and
//! the `/files` box).
//!
//! One table, two callers. The tree pane and the `/files` box used to draw
//! their own `▾ `/`▸ `/`  ` markers inline, so a change to one was not a
//! change to the other and a file type was distinguishable only by its
//! extension being visible. This module is the single answer to "what glyph
//! and what colour does this row get", for both surfaces — and, since the
//! indentation that says *which directory a row belongs to* was a third thing
//! each renderer computed on its own, to "how deep is this row drawn" too.
//!
//! ## Font support
//!
//! The default style is Nerd Font glyphs (`seti-`/`dev-`/`fa-`/`oct-` code
//! points), which is what makes the listing scannable the way a Yazi listing
//! is: `src` is a folder, `main.rs` is Rust, `README.md` is prose, and the eye
//! can find the file it wants without reading every extension.
//!
//! A terminal without a patched font renders those code points as tofu, so
//! [`FileIconStyle::Plain`] exists and is a complete, readable fallback — the
//! exact markers that shipped before. Select it with `CODEGG_FILE_ICONS=plain`
//! (also `ascii`, `none`, or `0`). It is an environment switch rather than a
//! config key on purpose: this is a terminal-capability question, not a user
//! preference the config schema should carry, and a wrong-shaped key in a
//! layered config discards more than it selects.
//!
//! The tree guide ([`indent_prefix`]) is deliberately **not** part of this
//! switch. It is box-drawing, which every terminal has, because "which
//! directory does this row belong to" cannot depend on a font the user may not
//! have — so plain and Nerd render the same hierarchy, only the icons differ.
//!
//! ## No I/O, no state
//!
//! Every function here is pure over `(kind, name, expanded, theme)`. The
//! active style is resolved once through a `OnceLock` so a repaint costs a
//! `Copy`, not an environment read.

use std::sync::OnceLock;

use ratatui::style::Style;

use crate::tui::app::state::file_tree::{FileTreeState, MAX_INDENT_DEPTH};
use crate::tui::file_tree::{TreeNodeKind, TreeRow};
use crate::tui::theme::Theme;

/// Environment switch selecting the icon style.
///
/// `plain`/`ascii`/`none`/`0` selects [`FileIconStyle::Plain`]; anything else
/// (including unset, which is what a user without the variable gets) selects
/// [`FileIconStyle::Nerd`].
pub const FILE_ICONS_ENV: &str = "CODEGG_FILE_ICONS";

/// Which glyph vocabulary a navigator renders with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileIconStyle {
    /// Nerd Font glyphs, categorized by file type.
    #[default]
    Nerd,
    /// Caret markers and no file-type glyphs — the pre-Nerd rendering.
    Plain,
}

impl FileIconStyle {
    /// Parse a raw switch value. Exposed so the mapping is testable without
    /// mutating the process environment.
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("plain" | "ascii" | "none" | "0" | "off" | "false") => Self::Plain,
            _ => Self::Nerd,
        }
    }

    /// The style this process renders with, resolved once.
    pub fn active() -> Self {
        static ACTIVE: OnceLock<FileIconStyle> = OnceLock::new();
        *ACTIVE.get_or_init(|| Self::parse(std::env::var(FILE_ICONS_ENV).ok().as_deref()))
    }
}

/// Glyphs. Nerd Font code points, with the fa-/oct- families preferred over
/// the dev-/seti- families wherever both exist: those ranges are present in
/// every Nerd Fonts release, so a user on an older patched font sees a glyph
/// rather than a hole.
mod glyph {
    pub const FOLDER: &str = "\u{f07b}";
    pub const FOLDER_OPEN: &str = "\u{f07c}";
    pub const FILE: &str = "\u{f15b}";
    pub const CONFIG: &str = "\u{f013}";
    pub const LOCK: &str = "\u{f023}";
    pub const BOOK: &str = "\u{f02d}";
    pub const DATABASE: &str = "\u{f1c0}";
    pub const IMAGE: &str = "\u{f1c5}";
    pub const PDF: &str = "\u{f1c1}";
    pub const ARCHIVE: &str = "\u{f1c6}";
    pub const TEXT: &str = "\u{f15c}";
    pub const AUDIO: &str = "\u{f1c7}";
    pub const VIDEO: &str = "\u{f1c8}";
    pub const FONT: &str = "\u{f031}";
    pub const FLASK: &str = "\u{f0c3}";
    pub const GIT: &str = "\u{f1d3}";
    pub const DOCKER: &str = "\u{f308}";
    pub const TERMINAL: &str = "\u{f489}";
    pub const GITHUB: &str = "\u{e709}";

    pub const RUST: &str = "\u{e7a8}";
    pub const GO: &str = "\u{e627}";
    pub const PYTHON: &str = "\u{e73c}";
    pub const JAVASCRIPT: &str = "\u{e74e}";
    pub const TYPESCRIPT: &str = "\u{e628}";
    pub const HTML: &str = "\u{e736}";
    pub const CSS: &str = "\u{e749}";
    pub const MARKDOWN: &str = "\u{e73e}";
    pub const C: &str = "\u{e61e}";
    pub const CPP: &str = "\u{e61d}";
    pub const CSHARP: &str = "\u{e648}";
    pub const JAVA: &str = "\u{e738}";
    pub const KOTLIN: &str = "\u{e634}";
    pub const SWIFT: &str = "\u{e755}";
    pub const PHP: &str = "\u{e73d}";
    pub const RUBY: &str = "\u{e739}";
    pub const LUA: &str = "\u{e620}";
    pub const SHELL: &str = TERMINAL;
    pub const MAKE: &str = "\u{f085}";

    /// Pre-Nerd markers, kept byte-identical to what shipped so the plain
    /// style is a true fallback rather than a different design.
    pub const CARET_OPEN: &str = "▾";
    pub const CARET_CLOSED: &str = "▸";
}

/// How a glyph is coloured, independent of any one theme.
///
/// The tone is resolved here and mapped to a theme color by [`icon_style`], so
/// both navigators agree on what "a code file looks like" without either of
/// them owning the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Directory,
    Code,
    Markup,
    Data,
    Media,
    Muted,
}

/// The glyph for one row, with no trailing separator.
///
/// A directory's glyph encodes expansion (`folder`/`folder-open`), which is
/// why there is no separate caret column: one cell says both what the entry
/// is and whether it is open. [`FileIconStyle::Plain`] keeps the caret.
pub fn glyph(kind: TreeNodeKind, name: &str, expanded: bool, style: FileIconStyle) -> &'static str {
    if style == FileIconStyle::Plain {
        return match (kind, expanded) {
            (TreeNodeKind::Directory, true) => glyph::CARET_OPEN,
            (TreeNodeKind::Directory, false) => glyph::CARET_CLOSED,
            (TreeNodeKind::File, _) => "",
        };
    }
    match kind {
        TreeNodeKind::Directory => match directory_glyph(name, expanded) {
            Some(found) => found,
            None if expanded => glyph::FOLDER_OPEN,
            None => glyph::FOLDER,
        },
        TreeNodeKind::File => file_glyph(name),
    }
}

/// The glyph and its colour for one row.
pub fn icon_style(
    kind: TreeNodeKind,
    name: &str,
    expanded: bool,
    style: FileIconStyle,
    theme: &Theme,
) -> (&'static str, Style) {
    (
        glyph(kind, name, expanded, style),
        row_style(kind, name, expanded, style, theme),
    )
}

/// The colour a row's glyph and name share.
///
/// The selected row is *not* styled here: selection is a surface-level
/// decision (`components::file_tree` / `dialogs::files`), and baking it into
/// the icon table would give the two navigators a second, disagreeing answer
/// to "what does the cursor look like".
pub fn row_style(
    kind: TreeNodeKind,
    name: &str,
    expanded: bool,
    style: FileIconStyle,
    theme: &Theme,
) -> Style {
    let color = if style == FileIconStyle::Plain {
        // The plain markers are structural, not decorative: they carried
        // `muted` before this module existed and must not change meaning.
        theme.muted
    } else {
        match tone(kind, name, expanded) {
            Tone::Directory => theme.primary,
            Tone::Code => theme.secondary,
            Tone::Markup => theme.link,
            Tone::Data => theme.warning,
            Tone::Media => theme.success,
            Tone::Muted => theme.muted,
        }
    };
    Style::default().fg(color)
}

/// Columns the icon span occupies: the glyph plus its trailing space.
///
/// Two, for both icon styles. A Nerd Font glyph is rendered double-width by
/// some terminals, so this over-reserves by one column there rather than
/// letting the guide push the name off a narrow column.
const ICON_COLS: usize = 2;

/// Columns one level of tree guide costs — `│  `, `├─ ` or `└─ `.
pub const INDENT_LEVEL_COLS: usize = 3;

/// The tree guide in front of one row, sized to what the column can spare.
///
/// One function, two callers, for the same reason the glyph table is: the
/// `/files` box and the Ctrl+T pane are the same navigator drawn twice, and
/// two copies of "how deep is this row" is how they came to disagree.
///
/// ## Why box-drawing and nothing else
///
/// `│` `├` `─` `└` are in the Basic Multilingual Plane and every terminal
/// draws them. The Nerd Font vocabulary is a *nice-to-have* for file types;
/// the hierarchy is load-bearing, so it must not depend on a patched font or
/// a user having found `CODEGG_FILE_ICONS=plain`. With the plain style a
/// nested row reads `├─ name` exactly as it does with the Nerd glyphs.
///
/// ## Why the guide is the thing that gives way
///
/// `budget` is what remains of the column once the name and the icon are
/// accounted for, and levels that do not fit are dropped from the **outside**
/// in: the guide to the immediate parent is what tells the reader what this
/// row belongs to, so that is the part that survives. When even one level does
/// not fit, the guide is empty and the name sits flush left — the name is
/// content, the guide is orientation.
pub fn tree_indent(
    state: &FileTreeState,
    rows: &[TreeRow],
    index: usize,
    inner_width: usize,
) -> String {
    let indent = state.indent_for(rows, index);
    // Char count rather than display width: it under-counts a CJK name, which
    // can leave the guide one column wider than the budget. Cheap and it
    // errs toward keeping the guide rather than dropping it.
    let name_cols = rows.get(index).map_or(0, |row| row.name.chars().count());
    indent_prefix(
        indent.depth,
        indent.last,
        inner_width.saturating_sub(name_cols + ICON_COLS),
    )
}

/// The guide for a row at `depth`, narrowed to `budget` columns.
///
/// `depth == 0` draws nothing: a top-level row has no parent to connect to.
pub fn indent_prefix(depth: usize, last: bool, budget: usize) -> String {
    let levels = depth.min(MAX_INDENT_DEPTH).min(budget / INDENT_LEVEL_COLS);
    let mut out = String::with_capacity(levels * INDENT_LEVEL_COLS);
    for level in 0..levels {
        out.push_str(match (level + 1 == levels, last) {
            (true, true) => "└─ ",
            (true, false) => "├─ ",
            (false, _) => "│  ",
        });
    }
    out
}

/// Icon for a directory that has a name of its own.
///
/// A handful of directory names carry more meaning than "folder" — a
/// `.github` directory is a GitHub workflow root, not a container. Matched on
/// the full lowercased name so `Docs` reads the same as `docs`.
fn directory_glyph(name: &str, _expanded: bool) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        ".github" => glyph::GITHUB,
        "docs" | "doc" | "documentation" => glyph::BOOK,
        "tests" | "test" | "spec" | "__tests__" => glyph::FLASK,
        "assets" | "static" | "images" | "img" => glyph::IMAGE,
        _ => return None,
    })
}

/// Icon for a file, chosen by whole name and then by extension.
///
/// Whole-name matches come first because the files that matter by name
/// (`Makefile`, `Dockerfile`, `LICENSE`) are the ones whose extension says
/// nothing. They override only the **glyph**: [`tone`] still classifies by
/// extension, so `Cargo.toml` can wear the Rust icon without claiming to be
/// source code.
fn file_glyph(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if let Some(found) = whole_name_glyph(&lower) {
        return found;
    }
    if lower.starts_with(".git") {
        return glyph::GIT;
    }
    extension_glyph(extension_key(&lower))
}

/// The extension a name is classified by, or `None` when it has none.
///
/// A dotfile is special-cased: its *name* starts at the first dot, so `.env`
/// is named `env` and `.env.example` is named `env.example` — neither is an
/// extension of a file whose stem is empty. Getting this wrong is what makes
/// a bare `.env`, the single most obviously a config file there is, render as
/// an unrecognized file.
fn extension_key(lower: &str) -> Option<&str> {
    if let Some(rest) = lower.strip_prefix('.') {
        return Some(rest.split_once('.').map_or(rest, |(head, _)| head));
    }
    match lower.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => Some(extension),
        _ => None,
    }
}

/// Icon for one extension, ignoring whole-name overrides.
fn extension_glyph(extension: Option<&str>) -> &'static str {
    let Some(extension) = extension else {
        return glyph::FILE;
    };
    match extension {
        // Systems
        "rs" => glyph::RUST,
        "go" => glyph::GO,
        "c" | "h" => glyph::C,
        "cc" | "cpp" | "cxx" | "c++" | "hpp" | "hh" | "hxx" => glyph::CPP,
        "cs" => glyph::CSHARP,
        "java" => glyph::JAVA,
        "kt" | "kts" => glyph::KOTLIN,
        "swift" => glyph::SWIFT,
        "php" => glyph::PHP,
        "rb" | "rake" | "gemspec" => glyph::RUBY,
        "lua" => glyph::LUA,
        "py" | "pyi" | "pyx" | "ipynb" => glyph::PYTHON,
        "js" | "mjs" | "cjs" | "jsx" => glyph::JAVASCRIPT,
        "ts" | "mts" | "cts" | "tsx" => glyph::TYPESCRIPT,
        "html" | "htm" | "vue" | "svelte" => glyph::HTML,
        "css" | "scss" | "sass" | "less" => glyph::CSS,
        "sh" | "bash" | "zsh" | "fish" | "nu" | "ps1" => glyph::SHELL,
        "mk" | "make" => glyph::MAKE,
        // Documents
        "md" | "mdx" | "markdown" | "rst" => glyph::MARKDOWN,
        "txt" | "text" | "log" => glyph::TEXT,
        "pdf" => glyph::PDF,
        // Data and configuration
        "json" | "jsonc" | "toml" | "yaml" | "yml" | "ini" | "cfg" | "conf" | "env"
        | "properties" => glyph::CONFIG,
        "xml" | "svg" | "plist" => glyph::HTML,
        "sql" | "sqlite" | "db" => glyph::DATABASE,
        "key" | "pem" | "crt" | "cer" | "p12" => glyph::LOCK,
        // Media
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "tif" | "tiff" | "avif" => {
            glyph::IMAGE
        }
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => glyph::AUDIO,
        "mp4" | "mov" | "mkv" | "webm" | "avi" => glyph::VIDEO,
        "ttf" | "otf" | "woff" | "woff2" | "eot" => glyph::FONT,
        // Archives
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "jar" | "wasm" => {
            glyph::ARCHIVE
        }
        _ => glyph::FILE,
    }
}

/// Icons for files whose *name* is the type.
fn whole_name_glyph(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "dockerfile" | "containerfile" | "docker-compose.yml" | "docker-compose.yaml" => {
            glyph::DOCKER
        }
        "makefile" | "gnumakefile" | "justfile" | "taskfile.yml" => glyph::MAKE,
        "license" | "license.md" | "licence" | "readme" | "changelog" | "authors" | "notice" => {
            glyph::BOOK
        }
        // The lock/manifest of a language reads as that language's project at
        // a glance. Glyph only: [`tone`] classifies these as configuration,
        // which is what they are.
        "cargo.lock" | "cargo.toml" | "package-lock.json" | "poetry.lock" | "uv.lock" => {
            glyph::RUST
        }
        _ => return None,
    })
}

/// The tone bucket a row falls into.
///
/// Classified from the **extension**, not from the glyph: a whole-name
/// override is about recognizability — that `Cargo.toml` should read as a
/// Rust project at a glance — not about what kind of thing the file is.
/// Feeding the glyph back in here painted every Rust manifest as source.
fn tone(kind: TreeNodeKind, name: &str, _expanded: bool) -> Tone {
    if kind == TreeNodeKind::Directory {
        return Tone::Directory;
    }
    let lower = name.to_ascii_lowercase();
    if lower.starts_with('.') {
        // A dotfile is configuration far more often than content, and a
        // recoloured `.` column reads calmer than a wall of accent color.
        return Tone::Data;
    }
    match extension_glyph(extension_key(&lower)) {
        glyph::RUST | glyph::GO | glyph::PYTHON | glyph::JAVASCRIPT | glyph::TYPESCRIPT => {
            Tone::Code
        }
        glyph::MARKDOWN | glyph::TEXT | glyph::HTML | glyph::CSS | glyph::SHELL => Tone::Markup,
        glyph::CONFIG | glyph::DATABASE | glyph::LOCK | glyph::MAKE | glyph::DOCKER => Tone::Data,
        glyph::IMAGE | glyph::AUDIO | glyph::VIDEO | glyph::FONT | glyph::PDF => Tone::Media,
        _ => Tone::Muted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nerd(kind: TreeNodeKind, name: &str) -> &'static str {
        glyph(kind, name, false, FileIconStyle::Nerd)
    }

    #[test]
    fn plain_is_the_pre_nerd_rendering_byte_for_byte() {
        assert_eq!(
            glyph(TreeNodeKind::Directory, "src", true, FileIconStyle::Plain),
            "▾"
        );
        assert_eq!(
            glyph(TreeNodeKind::Directory, "src", false, FileIconStyle::Plain),
            "▸"
        );
        assert_eq!(
            glyph(TreeNodeKind::File, "a.rs", false, FileIconStyle::Plain),
            ""
        );
    }

    #[test]
    fn directories_are_folders_and_expansion_is_visible_in_the_glyph() {
        assert_eq!(nerd(TreeNodeKind::Directory, "src"), glyph::FOLDER);
        assert_eq!(
            glyph(TreeNodeKind::Directory, "src", true, FileIconStyle::Nerd),
            glyph::FOLDER_OPEN,
            "an open directory must not look like a closed one"
        );
    }

    #[test]
    fn named_directories_get_their_own_icon() {
        assert_eq!(nerd(TreeNodeKind::Directory, ".github"), glyph::GITHUB);
        assert_eq!(nerd(TreeNodeKind::Directory, "Docs"), glyph::BOOK);
        assert_eq!(nerd(TreeNodeKind::Directory, "tests"), glyph::FLASK);
        assert_eq!(
            nerd(TreeNodeKind::Directory, "target"),
            glyph::FOLDER,
            "an unrecognized directory is still a folder"
        );
    }

    #[test]
    fn common_source_files_are_distinguishable_at_a_glance() {
        assert_eq!(nerd(TreeNodeKind::File, "main.rs"), glyph::RUST);
        assert_eq!(nerd(TreeNodeKind::File, "main.go"), glyph::GO);
        assert_eq!(nerd(TreeNodeKind::File, "app.tsx"), glyph::TYPESCRIPT);
        assert_eq!(nerd(TreeNodeKind::File, "app.js"), glyph::JAVASCRIPT);
        assert_eq!(nerd(TreeNodeKind::File, "run.sh"), glyph::SHELL);
        assert_eq!(
            nerd(TreeNodeKind::File, "lib.rs"),
            nerd(TreeNodeKind::File, "other.rs")
        );
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert_eq!(nerd(TreeNodeKind::File, "README.MD"), glyph::MARKDOWN);
        assert_eq!(nerd(TreeNodeKind::File, "Main.RS"), glyph::RUST);
    }

    #[test]
    fn dotfiles_and_whole_names_win_over_the_extension() {
        assert_eq!(nerd(TreeNodeKind::File, ".gitignore"), glyph::GIT);
        assert_eq!(nerd(TreeNodeKind::File, ".gitattributes"), glyph::GIT);
        assert_eq!(nerd(TreeNodeKind::File, "Dockerfile"), glyph::DOCKER);
        assert_eq!(nerd(TreeNodeKind::File, "Dockerfile.prod"), glyph::FILE);
        assert_eq!(
            nerd(TreeNodeKind::File, ".env"),
            glyph::CONFIG,
            "a dotfile has no stem, so the obvious reading is the only reading"
        );
        assert_eq!(nerd(TreeNodeKind::File, ".env.example"), glyph::CONFIG);
        assert_eq!(
            nerd(TreeNodeKind::File, ".bashrc"),
            glyph::FILE,
            "an unrecognized dotfile is still a dotfile, and the tone says config"
        );
    }

    #[test]
    fn a_whole_name_override_does_not_change_the_tone() {
        // `Cargo.toml` wears the Rust icon so a Rust project is recognizable
        // at a glance; it is still configuration, and coloring it as source
        // put a whole category of manifests in the wrong column of the eye.
        let theme = Theme::default();
        assert_eq!(
            nerd(TreeNodeKind::File, "Cargo.toml"),
            glyph::RUST,
            "named matches beat the extension"
        );
        assert_eq!(
            row_style(
                TreeNodeKind::File,
                "Cargo.toml",
                false,
                FileIconStyle::Nerd,
                &theme
            )
            .fg,
            row_style(
                TreeNodeKind::File,
                "pyproject.toml",
                false,
                FileIconStyle::Nerd,
                &theme
            )
            .fg,
        );
        assert_ne!(
            row_style(
                TreeNodeKind::File,
                "Cargo.toml",
                false,
                FileIconStyle::Nerd,
                &theme
            )
            .fg,
            row_style(
                TreeNodeKind::File,
                "main.rs",
                false,
                FileIconStyle::Nerd,
                &theme
            )
            .fg,
            "a manifest is not source code, whatever icon it wears"
        );
    }

    #[test]
    fn an_unknown_or_extensionless_file_is_a_plain_file() {
        assert_eq!(nerd(TreeNodeKind::File, "LICENSE"), glyph::BOOK);
        assert_eq!(nerd(TreeNodeKind::File, "Makefile"), glyph::MAKE);
        assert_eq!(nerd(TreeNodeKind::File, "Makefile.in"), glyph::FILE);
        assert_eq!(nerd(TreeNodeKind::File, "weird.qqq"), glyph::FILE);
        assert_eq!(nerd(TreeNodeKind::File, "noextension"), glyph::FILE);
    }

    #[test]
    fn tones_separate_directories_code_markup_and_data() {
        let theme = Theme::default();
        let color = |kind, name, style| {
            row_style(kind, name, false, style, &theme)
                .fg
                .expect("a row always has a colour")
        };
        let dir = color(TreeNodeKind::Directory, "src", FileIconStyle::Nerd);
        let code = color(TreeNodeKind::File, "main.rs", FileIconStyle::Nerd);
        let doc = color(TreeNodeKind::File, "README.md", FileIconStyle::Nerd);
        let data = color(TreeNodeKind::File, "Cargo.toml", FileIconStyle::Nerd);
        let dotfile = color(TreeNodeKind::File, ".bashrc", FileIconStyle::Nerd);
        assert_ne!(dir, code);
        assert_ne!(code, doc);
        assert_ne!(doc, data);
        assert_eq!(data, dotfile, "a dotfile reads as configuration");
    }

    #[test]
    fn the_plain_style_keeps_its_marker_muted_regardless_of_row() {
        let theme = Theme::default();
        let dir = row_style(
            TreeNodeKind::Directory,
            "src",
            false,
            FileIconStyle::Plain,
            &theme,
        );
        let file = row_style(
            TreeNodeKind::File,
            "main.rs",
            false,
            FileIconStyle::Plain,
            &theme,
        );
        assert_eq!(dir.fg, Some(theme.muted));
        assert_eq!(file.fg, Some(theme.muted));
    }

    #[test]
    fn the_env_switch_selects_the_fallback_and_defaults_to_nerd() {
        assert_eq!(FileIconStyle::parse(None), FileIconStyle::Nerd);
        assert_eq!(FileIconStyle::parse(Some("")), FileIconStyle::Nerd);
        assert_eq!(FileIconStyle::parse(Some("plain")), FileIconStyle::Plain);
        assert_eq!(FileIconStyle::parse(Some(" ASCII ")), FileIconStyle::Plain);
        assert_eq!(FileIconStyle::parse(Some("none")), FileIconStyle::Plain);
        assert_eq!(FileIconStyle::parse(Some("0")), FileIconStyle::Plain);
        assert_eq!(FileIconStyle::parse(Some("nerd")), FileIconStyle::Nerd);
        assert_eq!(
            FileIconStyle::parse(Some("tofu")),
            FileIconStyle::Nerd,
            "an unrecognized value must not silently disable icons"
        );
    }

    /// The guide ladder, as a table, because the shape is the contract: a top
    /// level row has no connector, a last child closes its level, and anything
    /// else hangs off a continuing one.
    #[test]
    fn the_guide_ladder_is_one_function_for_both_renderers() {
        assert_eq!(indent_prefix(0, false, 40), "", "top level: no guide");
        assert_eq!(
            indent_prefix(0, true, 40),
            "",
            "lastness is meaningless at depth 0"
        );
        assert_eq!(indent_prefix(1, false, 40), "├─ ");
        assert_eq!(indent_prefix(1, true, 40), "└─ ");
        assert_eq!(indent_prefix(2, false, 40), "│  ├─ ");
        assert_eq!(indent_prefix(3, true, 40), "│  │  └─ ");
    }

    /// A guide is orientation; a name is content. When the column cannot hold
    /// both, the guide is what gives way — including by going away entirely.
    #[test]
    fn a_narrow_column_elides_the_guide_before_the_name() {
        assert_eq!(indent_prefix(1, false, 0), "", "no room at all");
        assert_eq!(indent_prefix(1, false, 2), "", "less than one level");
        // Exactly one level: the connector to the immediate parent survives.
        assert_eq!(indent_prefix(4, false, 3), "├─ ");
        // More room than levels: nothing is invented.
        assert_eq!(
            indent_prefix(1, false, 40).chars().count(),
            INDENT_LEVEL_COLS,
            "box drawing is single-width, so columns are characters"
        );
        // Never wider than the budget, at any depth.
        for depth in 0..12 {
            for budget in 0..24 {
                let guide = indent_prefix(depth, true, budget);
                assert!(
                    guide.chars().count() <= budget,
                    "depth {depth} drew {guide:?} ({}) into {budget} columns",
                    guide.chars().count()
                );
            }
        }
    }

    /// Depths past the cap are drawn, not rejected — the cap is on *columns*,
    /// and a listing may legitimately be deeper than any single column is wide.
    #[test]
    fn a_deep_scope_is_clamped_to_one_cap_rather_than_unbounded() {
        let capped = indent_prefix(MAX_INDENT_DEPTH + 5, true, 1_000);
        let at_cap = indent_prefix(MAX_INDENT_DEPTH, true, 1_000);
        assert_eq!(capped, at_cap);
        assert_eq!(at_cap.chars().count(), MAX_INDENT_DEPTH * INDENT_LEVEL_COLS);
    }

    /// The guide must not depend on the icon style, or `CODEGG_FILE_ICONS=plain`
    /// would trade away the hierarchy along with the glyphs.
    #[test]
    fn the_guide_is_box_drawing_and_needs_no_nerd_font() {
        for glyph in ['│', '├', '─', '└'] {
            let code = glyph as u32;
            assert!(
                !glyph.is_ascii(),
                "{glyph:?} must be box drawing, not an ASCII stand-in"
            );
            assert!(
                !(0xE000..=0xF8FF).contains(&code),
                "{glyph:?} is in the Nerd Font private-use range"
            );
        }
    }

    /// End to end through the state: real scope, drawn from the same
    /// `tree_indent` both surfaces call.
    #[test]
    fn tree_indent_counts_the_scope_a_subtree_walk_restarted() {
        use crate::tui::app::state::file_tree::FileTreeState;
        use crate::tui::file_tree::TreeRow;

        let row = |path: &str, name: &str, depth: usize| TreeRow {
            path: path.to_string(),
            name: name.to_string(),
            depth,
            kind: TreeNodeKind::File,
            expanded: false,
        };
        let rows = vec![row("src", "src", 0), row("src/lib.rs", "lib.rs", 1)];

        let mut state = FileTreeState::default();
        assert_eq!(state.indent_for(&rows, 0).depth, 0, "at the root, nothing");
        assert_eq!(state.indent_for(&rows, 1).depth, 1);
        assert_eq!(tree_indent(&state, &rows, 0, 40), "");
        assert_eq!(
            tree_indent(&state, &rows, 1, 40),
            "└─ ",
            "one level in: the connector to the parent, and nothing above it"
        );

        // Two levels: the outer level keeps a continuing guide so the row is
        // visibly inside something rather than merely indented.
        let deeper = vec![row("a", "a", 0), row("a/b", "b", 1), row("a/b/c", "c", 2)];
        assert_eq!(
            tree_indent(&state, &deeper, 1, 40),
            "├─ ",
            "one level in, with a sibling still to come, keeps its level open"
        );
        assert_eq!(
            tree_indent(&state, &deeper, 2, 40),
            "│  └─ ",
            "two levels in and last: the outer level continues, this one closes"
        );

        assert!(state.set_root_prefix("src"));
        // The indent is **relative to the scope**. A subtree walk restarts the
        // depth count at the subtree, so a scoped listing's top level carries
        // the same number the workspace root's top level carries — and adding
        // the scope's own depth back on top of it is what used to make a
        // scoped listing render identically to the root, and what flattened
        // every deep scope to the same cap.
        let scoped = vec![row("src/lib.rs", "lib.rs", 1)];
        assert_eq!(
            state.indent_for(&scoped, 0).depth,
            1,
            "the indent is relative to the scope, so the scope's own depth is \
             not added back"
        );
        assert_eq!(
            tree_indent(&state, &scoped, 0, 40),
            "└─ ",
            "the scope itself is the parent, so its only child closes the level"
        );
    }
}
