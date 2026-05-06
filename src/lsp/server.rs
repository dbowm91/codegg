use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct LspServerDef {
    pub id: &'static str,
    pub languages: &'static [&'static str],
    pub extensions: &'static [&'static str],
    pub repo: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub download: Option<DownloadSpec>,
}

#[derive(Debug, Clone)]
pub struct DownloadSpec {
    pub url_template: &'static str,
    pub archive_type: ArchiveType,
    pub binary_name: &'static str,
}

#[derive(Debug, Clone)]
pub enum ArchiveType {
    Zip,
    TarGz,
    TarXz,
    Raw,
}

pub fn server_definitions() -> &'static [LspServerDef] {
    &[
        LspServerDef {
            id: "rust-analyzer",
            languages: &["rust"],
            extensions: &["rs"],
            repo: "rust-lang/rust-analyzer",
            command: "rust-analyzer",
            args: &[],
            download: Some(DownloadSpec {
                url_template: "https://github.com/rust-lang/rust-analyzer/releases/latest/download/rust-analyzer-{arch}-{os}.gz",
                archive_type: ArchiveType::Raw,
                binary_name: "rust-analyzer",
            }),
        },
        LspServerDef {
            id: "gopls",
            languages: &["go"],
            extensions: &["go"],
            repo: "golang/tools/gopls",
            command: "gopls",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "pyright",
            languages: &["python"],
            extensions: &["py", "pyw"],
            repo: "microsoft/pyright",
            command: "pyright-langserver",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "typescript-language-server",
            languages: &["javascript", "javascriptreact", "typescript", "typescriptreact"],
            extensions: &["js", "jsx", "ts", "tsx"],
            repo: "typescript-language-server/typescript-language-server",
            command: "typescript-language-server",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "jdtls",
            languages: &["java"],
            extensions: &["java"],
            repo: "eclipse-jdtls/eclipse.jdt.ls",
            command: "jdtls",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "clangd",
            languages: &["c", "cpp", "objective-c", "objective-cpp"],
            extensions: &["c", "h", "cpp", "cc", "cxx", "hpp", "hxx", "m", "mm"],
            repo: "clangd/clangd",
            command: "clangd",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "omnisharp",
            languages: &["csharp"],
            extensions: &["cs"],
            repo: "OmniSharp/omnisharp-roslyn",
            command: "OmniSharp",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "kotlin-language-server",
            languages: &["kotlin"],
            extensions: &["kt", "kts"],
            repo: "fwcd/kotlin-language-server",
            command: "kotlin-language-server",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "lua-language-server",
            languages: &["lua"],
            extensions: &["lua"],
            repo: "LuaLS/lua-language-server",
            command: "lua-language-server",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "haskell-language-server",
            languages: &["haskell"],
            extensions: &["hs", "lhs"],
            repo: "haskell/haskell-language-server",
            command: "haskell-language-server-wrapper",
            args: &["--lsp"],
            download: None,
        },
        LspServerDef {
            id: "metals",
            languages: &["scala"],
            extensions: &["scala", "sc"],
            repo: "scalameta/metals",
            command: "metals",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "elixir-ls",
            languages: &["elixir"],
            extensions: &["ex", "exs"],
            repo: "elixir-lsp/elixir-ls",
            command: "elixir-ls",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "clojure-lsp",
            languages: &["clojure"],
            extensions: &["clj", "cljs", "cljc"],
            repo: "clojure-lsp/clojure-lsp",
            command: "clojure-lsp",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "vue-language-server",
            languages: &["vue"],
            extensions: &["vue"],
            repo: "vuejs/language-tools",
            command: "vue-language-server",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "svelte-language-server",
            languages: &["svelte"],
            extensions: &["svelte"],
            repo: "sveltejs/language-tools",
            command: "svelteserver",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "yaml-language-server",
            languages: &["yaml"],
            extensions: &["yaml", "yml"],
            repo: "redhat-developer/yaml-language-server",
            command: "yaml-language-server",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "taplo",
            languages: &["toml"],
            extensions: &["toml"],
            repo: "tamasfe/taplo",
            command: "taplo",
            args: &["lsp", "stdio"],
            download: None,
        },
        LspServerDef {
            id: "bash-language-server",
            languages: &["shellscript"],
            extensions: &["sh", "bash", "zsh", "fish"],
            repo: "bash-lsp/bash-language-server",
            command: "bash-language-server",
            args: &["start"],
            download: None,
        },
        LspServerDef {
            id: "terraform-ls",
            languages: &["terraform"],
            extensions: &["tf", "tfvars"],
            repo: "hashicorp/terraform-ls",
            command: "terraform-ls",
            args: &["serve"],
            download: None,
        },
        LspServerDef {
            id: "zls",
            languages: &["zig"],
            extensions: &["zig"],
            repo: "zigtools/zls",
            command: "zls",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "marksman",
            languages: &["markdown"],
            extensions: &["md"],
            repo: "artempyanykh/marksman",
            command: "marksman",
            args: &["server"],
            download: None,
        },
        LspServerDef {
            id: "dockerfile-language-server",
            languages: &["dockerfile"],
            extensions: &[] /* special name */,
            repo: "rcjsuen/dockerfile-language-server-nodejs",
            command: "docker-langserver",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "sql-language-server",
            languages: &["sql"],
            extensions: &["sql"],
            repo: "joe-re/sql-language-server",
            command: "sql-language-server",
            args: &["up", "--method", "stdio"],
            download: None,
        },
        LspServerDef {
            id: "ruby-lsp",
            languages: &["ruby"],
            extensions: &["rb"],
            repo: "Shopify/ruby-lsp",
            command: "ruby-lsp",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "php-language-server",
            languages: &["php"],
            extensions: &["php"],
            repo: "felixfbecker/php-language-server",
            command: "php-language-server",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "swift-sourcekit",
            languages: &["swift"],
            extensions: &["swift"],
            repo: "apple/sourcekit-lsp",
            command: "sourcekit-lsp",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "dart-analysis-server",
            languages: &["dart"],
            extensions: &["dart"],
            repo: "dart-lang/sdk",
            command: "dart",
            args: &["language-server", "--client-id", "codegg"],
            download: None,
        },
        LspServerDef {
            id: "erlang-ls",
            languages: &["erlang"],
            extensions: &["erl", "hrl"],
            repo: "erlang-ls/erlang_ls",
            command: "erlang_ls",
            args: &[],
            download: None,
        },
        LspServerDef {
            id: "html-language-server",
            languages: &["html"],
            extensions: &["html", "htm"],
            repo: "vscode-langservers/vscode-html-languageserver-bin",
            command: "vscode-html-language-server",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "css-language-server",
            languages: &["css", "scss", "less"],
            extensions: &["css", "scss", "less"],
            repo: "vscode-langservers/vscode-css-languageserver-bin",
            command: "vscode-css-language-server",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "json-language-server",
            languages: &["json", "jsonc"],
            extensions: &["json", "jsonc"],
            repo: "vscode-langservers/vscode-json-languageserver-bin",
            command: "vscode-json-language-server",
            args: &["--stdio"],
            download: None,
        },
        LspServerDef {
            id: "solidity-language-server",
            languages: &["solidity"],
            extensions: &["sol"],
            repo: "NomicFoundation/hardhat-vscode",
            command: "nomicfoundation-solidity-language-server",
            args: &["--stdio"],
            download: None,
        },
    ]
}

pub fn find_server(id: &str) -> Option<&'static LspServerDef> {
    server_definitions().iter().find(|s| s.id == id)
}

pub fn find_server_for_language(lang: &str) -> Option<&'static LspServerDef> {
    server_definitions()
        .iter()
        .find(|s| s.languages.contains(&lang))
}

pub fn find_server_for_extension(ext: &str) -> Option<&'static LspServerDef> {
    server_definitions()
        .iter()
        .find(|s| s.extensions.contains(&ext))
}

pub fn build_env_overrides(env: Option<&HashMap<String, String>>) -> Vec<(String, String)> {
    env.map(|e| {
        e.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>()
    })
    .unwrap_or_default()
}
