use std::path::{Path, PathBuf};

use tracing::info;

pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start
        .canonicalize()
        .ok()
        .unwrap_or_else(|| start.to_path_buf());

    loop {
        if is_project_root(&current) {
            info!(path = ?current, "found project root");
            return Some(current.clone());
        }

        if !current.pop() {
            break;
        }
    }

    let fallback = start
        .canonicalize()
        .ok()
        .unwrap_or_else(|| start.to_path_buf());
    info!(path = ?fallback, "using directory as project root");
    Some(fallback)
}

fn is_project_root(dir: &Path) -> bool {
    let markers = [
        ".git",
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pyproject.toml",
        "setup.py",
        "requirements.txt",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "CMakeLists.txt",
        "Makefile",
        "Gemfile",
        "composer.json",
        "mix.exs",
        "rebar.config",
        "project.clj",
        "pubspec.yaml",
        "Package.swift",
        "terraform.tf",
        "main.tf",
        ".terraform",
        "flake.nix",
        "shell.nix",
        "default.nix",
        "stack.yaml",
        "cabal.project",
        "dune-project",
        ".bazelrc",
        "WORKSPACE",
        "BUILD.bazel",
        ".luarc.json",
        ".luacheckrc",
        "tsconfig.json",
        "jsconfig.json",
        ".eslintrc",
        ".eslintrc.json",
        ".eslintrc.js",
        ".prettierrc",
        "vite.config.ts",
        "vite.config.js",
        "next.config.js",
        "next.config.ts",
        "nuxt.config.ts",
        "angular.json",
        ".svelte-kit",
        "astro.config.mjs",
        "remix.config.js",
        "gatsby-config.js",
        ".dockerignore",
        "Dockerfile",
        "docker-compose.yml",
        "docker-compose.yaml",
        ".github",
        ".gitlab-ci.yml",
        ".circleci",
        "Jenkinsfile",
        ".pre-commit-config.yaml",
    ];

    markers.iter().any(|m| dir.join(m).exists())
}
