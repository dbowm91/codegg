//! Compatibility re-exports for the canonical Git process policy.

pub use egggit::process::{GitEnvPolicy, ALLOWED_ENV_VARS, ALWAYS_STRIPPED_ENV_VARS};

pub fn is_allowed(name: &str) -> bool {
    ALLOWED_ENV_VARS.contains(&name)
}

pub fn is_stripped(name: &str) -> bool {
    ALWAYS_STRIPPED_ENV_VARS.contains(&name)
}
