pub fn extension_to_language_id(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("rust"),
        "go" => Some("go"),
        "py" => Some("python"),
        "pyw" => Some("python"),
        "pyx" => Some("python"),
        "js" => Some("javascript"),
        "jsx" => Some("javascriptreact"),
        "ts" => Some("typescript"),
        "tsx" => Some("typescriptreact"),
        "java" => Some("java"),
        "kt" => Some("kotlin"),
        "kts" => Some("kotlin"),
        "c" => Some("c"),
        "h" => Some("c"),
        "cpp" => Some("cpp"),
        "cc" => Some("cpp"),
        "cxx" => Some("cpp"),
        "hpp" => Some("cpp"),
        "hxx" => Some("cpp"),
        "cs" => Some("csharp"),
        "php" => Some("php"),
        "rb" => Some("ruby"),
        "swift" => Some("swift"),
        "m" => Some("objective-c"),
        "mm" => Some("objective-cpp"),
        "lua" => Some("lua"),
        "pl" => Some("perl"),
        "pm" => Some("perl"),
        "raku" => Some("raku"),
        "hs" => Some("haskell"),
        "lhs" => Some("haskell"),
        "scala" => Some("scala"),
        "sc" => Some("scala"),
        "dart" => Some("dart"),
        "ex" => Some("elixir"),
        "exs" => Some("elixir"),
        "erl" => Some("erlang"),
        "hrl" => Some("erlang"),
        "clj" => Some("clojure"),
        "cljs" => Some("clojure"),
        "cljc" => Some("clojure"),
        "vue" => Some("vue"),
        "svelte" => Some("svelte"),
        "html" => Some("html"),
        "htm" => Some("html"),
        "css" => Some("css"),
        "scss" => Some("scss"),
        "sass" => Some("sass"),
        "less" => Some("less"),
        "json" => Some("json"),
        "jsonc" => Some("jsonc"),
        "yaml" => Some("yaml"),
        "yml" => Some("yaml"),
        "toml" => Some("toml"),
        "xml" => Some("xml"),
        "sh" => Some("shellscript"),
        "bash" => Some("shellscript"),
        "zsh" => Some("shellscript"),
        "fish" => Some("shellscript"),
        "ps1" => Some("powershell"),
        "psm1" => Some("powershell"),
        "psd1" => Some("powershell"),
        "sql" => Some("sql"),
        "graphql" => Some("graphql"),
        "gql" => Some("graphql"),
        "proto" => Some("proto"),
        "tf" => Some("terraform"),
        "tfvars" => Some("terraform"),
        "dockerfile" => Some("dockerfile"),
        "md" => Some("markdown"),
        "r" => Some("r"),
        "R" => Some("r"),
        "zig" => Some("zig"),
        "nim" => Some("nim"),
        "v" => Some("v"),
        "sol" => Some("solidity"),
        "makefile" => Some("makefile"),
        "cmake" => Some("cmake"),
        _ => None,
    }
}

pub fn language_id_to_server_id(lang_id: &str) -> Option<&'static str> {
    match lang_id {
        "rust" => Some("rust-analyzer"),
        "go" => Some("gopls"),
        "python" => Some("pyright"),
        "javascript" | "javascriptreact" => Some("typescript-language-server"),
        "typescript" | "typescriptreact" => Some("typescript-language-server"),
        "java" => Some("jdtls"),
        "kotlin" => Some("kotlin-language-server"),
        "c" | "cpp" => Some("clangd"),
        "csharp" => Some("omnisharp"),
        "php" => Some("php-language-server"),
        "ruby" => Some("ruby-lsp"),
        "swift" => Some("sourcekit-lsp"),
        "objective-c" | "objective-cpp" => Some("clangd"),
        "lua" => Some("lua-language-server"),
        "perl" | "raku" => Some("perl-language-server"),
        "haskell" => Some("haskell-language-server"),
        "scala" => Some("metals"),
        "dart" => Some("dart-analysis-server"),
        "elixir" => Some("elixir-ls"),
        "erlang" => Some("erlang-ls"),
        "clojure" => Some("clojure-lsp"),
        "vue" => Some("vue-language-server"),
        "svelte" => Some("svelte-language-server"),
        "html" => Some("html-language-server"),
        "css" | "scss" | "sass" | "less" => Some("css-language-server"),
        "json" | "jsonc" => Some("json-language-server"),
        "yaml" => Some("yaml-language-server"),
        "toml" => Some("taplo"),
        "xml" => Some("lemminx"),
        "shellscript" => Some("bash-language-server"),
        "powershell" => Some("powershell-editor-services"),
        "sql" => Some("sql-language-server"),
        "graphql" => Some("graphql-language-server"),
        "proto" => Some("buf-language-server"),
        "terraform" => Some("terraform-ls"),
        "dockerfile" => Some("dockerfile-language-server"),
        "markdown" => Some("marksman"),
        "r" => Some("r-languageserver"),
        "zig" => Some("zls"),
        "nim" => Some("nimlsp"),
        "v" => Some("vls"),
        "solidity" => Some("solidity-language-server"),
        "makefile" => Some("makefile-language-server"),
        "cmake" => Some("cmake-language-server"),
        _ => None,
    }
}

pub fn detect_language(path: &str) -> Option<&'static str> {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let filename = filename.rsplit('\\').next().unwrap_or(filename);

    let ext = if let Some(idx) = filename.rfind('.') {
        &filename[idx + 1..]
    } else {
        match filename.to_lowercase().as_str() {
            "dockerfile" => return extension_to_language_id("dockerfile"),
            "makefile" => return extension_to_language_id("makefile"),
            _ => return None,
        }
    };
    extension_to_language_id(ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_rust() {
        assert_eq!(extension_to_language_id("rs"), Some("rust"));
    }

    #[test]
    fn extension_python() {
        assert_eq!(extension_to_language_id("py"), Some("python"));
    }

    #[test]
    fn extension_unknown() {
        assert!(extension_to_language_id("unknown_xyz").is_none());
    }

    #[test]
    fn language_to_server() {
        assert_eq!(language_id_to_server_id("rust"), Some("rust-analyzer"));
        assert_eq!(language_id_to_server_id("go"), Some("gopls"));
    }

    #[test]
    fn detect_language_with_path() {
        assert_eq!(detect_language("src/main.rs"), Some("rust"));
        assert_eq!(detect_language("a/b/c.py"), Some("python"));
    }

    #[test]
    fn detect_language_dockerfile() {
        assert_eq!(detect_language("Dockerfile"), Some("dockerfile"));
    }
}
