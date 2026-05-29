use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyEcosystem {
    RustCargo,
    NodeNpm,
    PythonPip,
    PythonPoetry,
    Docker,
    GithubActions,
    Unknown,
}

pub fn detect_dependency_file(path: &Path) -> Option<DependencyEcosystem> {
    let file_name = path.file_name().and_then(|n| n.to_str())?;
    let path_str = path.to_string_lossy();

    match file_name {
        "Cargo.toml" | "Cargo.lock" => Some(DependencyEcosystem::RustCargo),
        "package.json" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => {
            Some(DependencyEcosystem::NodeNpm)
        }
        "requirements.txt" | "setup.py" | "setup.cfg" => {
            // Check if poetry.lock exists in the same directory
            let parent = path.parent()?;
            if parent.join("poetry.lock").exists() {
                Some(DependencyEcosystem::PythonPoetry)
            } else {
                Some(DependencyEcosystem::PythonPip)
            }
        }
        "pyproject.toml" => {
            // Check if poetry.lock exists in the same directory
            let parent = path.parent()?;
            if parent.join("poetry.lock").exists() {
                Some(DependencyEcosystem::PythonPoetry)
            } else {
                Some(DependencyEcosystem::PythonPip)
            }
        }
        "Dockerfile" | "docker-compose.yml" | "docker-compose.yaml" => {
            Some(DependencyEcosystem::Docker)
        }
        _ => {
            // Check for .github/workflows/*.yml
            if path_str.starts_with(".github/workflows/")
                && (file_name.ends_with(".yml") || file_name.ends_with(".yaml"))
            {
                Some(DependencyEcosystem::GithubActions)
            } else {
                None
            }
        }
    }
}

pub fn recommended_audit_commands(ecosystem: DependencyEcosystem) -> Vec<String> {
    match ecosystem {
        DependencyEcosystem::RustCargo => {
            vec![
                "cargo audit".to_string(),
                "cargo deny check advisories".to_string(),
            ]
        }
        DependencyEcosystem::NodeNpm => vec!["npm audit --json".to_string()],
        DependencyEcosystem::PythonPip | DependencyEcosystem::PythonPoetry => {
            vec!["pip-audit -f json".to_string()]
        }
        DependencyEcosystem::Docker
        | DependencyEcosystem::GithubActions
        | DependencyEcosystem::Unknown => {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detect_cargo_toml() {
        let p = PathBuf::from("Cargo.toml");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::RustCargo)
        );
    }

    #[test]
    fn detect_cargo_lock() {
        let p = PathBuf::from("Cargo.lock");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::RustCargo)
        );
    }

    #[test]
    fn detect_package_json() {
        let p = PathBuf::from("package.json");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::NodeNpm)
        );
    }

    #[test]
    fn detect_package_lock_json() {
        let p = PathBuf::from("package-lock.json");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::NodeNpm)
        );
    }

    #[test]
    fn detect_yarn_lock() {
        let p = PathBuf::from("yarn.lock");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::NodeNpm)
        );
    }

    #[test]
    fn detect_pnpm_lock() {
        let p = PathBuf::from("pnpm-lock.yaml");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::NodeNpm)
        );
    }

    #[test]
    fn detect_requirements_txt() {
        let p = PathBuf::from("requirements.txt");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::PythonPip)
        );
    }

    #[test]
    fn detect_setup_py() {
        let p = PathBuf::from("setup.py");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::PythonPip)
        );
    }

    #[test]
    fn detect_pyproject_toml() {
        let p = PathBuf::from("pyproject.toml");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::PythonPip)
        );
    }

    #[test]
    fn detect_pyproject_toml_with_poetry_lock() {
        let dir = tempfile::tempdir().unwrap();
        let pyproject = dir.path().join("pyproject.toml");
        std::fs::write(&pyproject, "[tool.poetry]").unwrap();
        let poetry_lock = dir.path().join("poetry.lock");
        std::fs::write(&poetry_lock, "").unwrap();
        assert_eq!(
            detect_dependency_file(&pyproject),
            Some(DependencyEcosystem::PythonPoetry)
        );
    }

    #[test]
    fn detect_requirements_txt_with_poetry_lock() {
        let dir = tempfile::tempdir().unwrap();
        let req = dir.path().join("requirements.txt");
        std::fs::write(&req, "flask").unwrap();
        let poetry_lock = dir.path().join("poetry.lock");
        std::fs::write(&poetry_lock, "").unwrap();
        assert_eq!(
            detect_dependency_file(&req),
            Some(DependencyEcosystem::PythonPoetry)
        );
    }

    #[test]
    fn detect_dockerfile() {
        let p = PathBuf::from("Dockerfile");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::Docker)
        );
    }

    #[test]
    fn detect_docker_compose_yml() {
        let p = PathBuf::from("docker-compose.yml");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::Docker)
        );
    }

    #[test]
    fn detect_docker_compose_yaml() {
        let p = PathBuf::from("docker-compose.yaml");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::Docker)
        );
    }

    #[test]
    fn detect_github_workflow() {
        let p = PathBuf::from(".github/workflows/ci.yml");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::GithubActions)
        );
    }

    #[test]
    fn detect_github_workflow_yaml() {
        let p = PathBuf::from(".github/workflows/deploy.yaml");
        assert_eq!(
            detect_dependency_file(&p),
            Some(DependencyEcosystem::GithubActions)
        );
    }

    #[test]
    fn detect_unknown_for_regular_file() {
        let p = PathBuf::from("src/main.rs");
        assert_eq!(detect_dependency_file(&p), None);
    }

    #[test]
    fn detect_unknown_for_readme() {
        let p = PathBuf::from("README.md");
        assert_eq!(detect_dependency_file(&p), None);
    }

    #[test]
    fn rust_cargo_audit_commands() {
        let cmds = recommended_audit_commands(DependencyEcosystem::RustCargo);
        assert_eq!(cmds.len(), 2);
        assert!(cmds.contains(&"cargo audit".to_string()));
        assert!(cmds.contains(&"cargo deny check advisories".to_string()));
    }

    #[test]
    fn node_npm_audit_commands() {
        let cmds = recommended_audit_commands(DependencyEcosystem::NodeNpm);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0], "npm audit --json");
    }

    #[test]
    fn python_pip_audit_commands() {
        let cmds = recommended_audit_commands(DependencyEcosystem::PythonPip);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0], "pip-audit -f json");
    }

    #[test]
    fn python_poetry_audit_commands() {
        let cmds = recommended_audit_commands(DependencyEcosystem::PythonPoetry);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0], "pip-audit -f json");
    }

    #[test]
    fn docker_no_audit_commands() {
        let cmds = recommended_audit_commands(DependencyEcosystem::Docker);
        assert!(cmds.is_empty());
    }

    #[test]
    fn github_actions_no_audit_commands() {
        let cmds = recommended_audit_commands(DependencyEcosystem::GithubActions);
        assert!(cmds.is_empty());
    }

    #[test]
    fn unknown_no_audit_commands() {
        let cmds = recommended_audit_commands(DependencyEcosystem::Unknown);
        assert!(cmds.is_empty());
    }

    #[test]
    fn ecosystem_serialization_roundtrip() {
        let ecosystems = [
            DependencyEcosystem::RustCargo,
            DependencyEcosystem::NodeNpm,
            DependencyEcosystem::PythonPip,
            DependencyEcosystem::PythonPoetry,
            DependencyEcosystem::Docker,
            DependencyEcosystem::GithubActions,
            DependencyEcosystem::Unknown,
        ];
        for eco in &ecosystems {
            let json = serde_json::to_string(eco).unwrap();
            let deserialized: DependencyEcosystem = serde_json::from_str(&json).unwrap();
            assert_eq!(*eco, deserialized);
        }
    }
}
