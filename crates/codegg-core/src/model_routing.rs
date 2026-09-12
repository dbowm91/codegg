//! Codegg's adapter for the neutral EggPool semantic model-routing policy.
//!
//! Configuration parsing stays in `codegg-config`; this module performs the
//! one-way translation into the shared compiler. It contains no provider,
//! session, credential, or inference behavior. The root agent runtime owns
//! selector execution and concrete provider invocation.

use std::collections::BTreeMap;

use codegg_config::schema::{Config, ModelRouteConfig, ModelRouterConfig, VIRTUAL_MODEL_PREFIX};
pub use eggpool_model_routing::{
    session_identity_from_header, CompiledModelRoute, CompiledModelRouter, ModelRouterRegistry,
    ModelRoutingError, SessionIdentity, SessionSource,
};

pub fn is_virtual_model(model: &str) -> bool {
    model.starts_with(VIRTUAL_MODEL_PREFIX)
}

/// Compile all configured semantic routers using the shared structural and
/// deterministic policy implementation.
pub fn compile_model_routers(config: &Config) -> Result<ModelRouterRegistry, ModelRoutingError> {
    let policies = config
        .model_routers
        .as_ref()
        .map(|routers| {
            routers
                .iter()
                .map(|(virtual_model, config)| (virtual_model.clone(), to_shared_policy(config)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    ModelRouterRegistry::from_policies(&policies)
}

fn to_shared_policy(config: &ModelRouterConfig) -> eggpool_model_routing::ModelRouterPolicy {
    eggpool_model_routing::ModelRouterPolicy {
        selector_model: config.selector_model.clone(),
        default_model: config.default_model.clone(),
        routes: config
            .routes
            .iter()
            .map(|(label, route)| (label.clone(), to_shared_route(route)))
            .collect(),
        sticky: config.sticky,
        affinity_ttl_s: config.affinity_ttl_s,
        selector_timeout_s: config.selector_timeout_s,
        max_input_bytes: config.max_input_bytes,
        repair_attempts: config.repair_attempts,
    }
}

fn to_shared_route(route: &ModelRouteConfig) -> eggpool_model_routing::ModelRoutePolicy {
    eggpool_model_routing::ModelRoutePolicy {
        model: route.model.clone(),
        description: route.description.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_config::schema::{ModelRouteConfig, ModelRouterConfig};
    use std::collections::HashMap;

    fn config() -> Config {
        Config {
            model_routers: Some(HashMap::from([(
                "virtual:code".to_string(),
                ModelRouterConfig {
                    selector_model: "openai/gpt-4o-mini".to_string(),
                    default_model: "anthropic/claude-sonnet".to_string(),
                    routes: HashMap::from([
                        (
                            "fast".to_string(),
                            ModelRouteConfig {
                                model: "openai/gpt-4o-mini".to_string(),
                                description: "short low-risk task".to_string(),
                            },
                        ),
                        (
                            "deep".to_string(),
                            ModelRouteConfig {
                                model: "anthropic/claude-sonnet".to_string(),
                                description: "deep coding task".to_string(),
                            },
                        ),
                    ]),
                    sticky: false,
                    affinity_ttl_s: 60.0,
                    selector_timeout_s: 1.0,
                    max_input_bytes: 512,
                    repair_attempts: 1,
                },
            )])),
            ..Default::default()
        }
    }

    #[test]
    fn adapter_compiles_codegg_config_with_shared_semantics() {
        let registry = compile_model_routers(&config()).expect("policy should compile");
        let router = registry.get("virtual:code").expect("router should exist");
        assert_eq!(router.selector_model, "openai/gpt-4o-mini");
        assert_eq!(router.default_model, "anthropic/claude-sonnet");
        assert_eq!(router.routes.len(), 2);
        assert_eq!(router.route_for_id("0").unwrap().label, "deep");
        assert_eq!(router.route_for_id("1").unwrap().label, "fast");
        assert_eq!(router.max_input_bytes, 512);
    }

    #[test]
    fn canonical_policy_and_identity_vectors_match_eggpool() {
        let config = Config {
            model_routers: Some(HashMap::from([(
                "virtual-route".to_string(),
                ModelRouterConfig {
                    selector_model: "selector-model".to_string(),
                    default_model: "model-default".to_string(),
                    routes: HashMap::from([
                        (
                            "z-fast".to_string(),
                            ModelRouteConfig {
                                model: "model-fast".to_string(),
                                description: " Fast\tpath ".to_string(),
                            },
                        ),
                        (
                            "a-default".to_string(),
                            ModelRouteConfig {
                                model: "model-default".to_string(),
                                description: "Default\npath".to_string(),
                            },
                        ),
                    ]),
                    sticky: true,
                    affinity_ttl_s: 60.0,
                    selector_timeout_s: 2.0,
                    max_input_bytes: 256,
                    repair_attempts: 1,
                },
            )])),
            ..Default::default()
        };
        let registry = compile_model_routers(&config).expect("canonical policy compiles");
        let router = registry.get("virtual-route").expect("canonical router");
        assert_eq!(
            router.static_policy.as_ref(),
            b"model-router/v1|choose id;reply id only|0=Default path|1=Fast path"
        );
        assert_eq!(
            router.config_fingerprint,
            "70c26421aa06f8d476e158e3a9f477526d5dc80eccb8634bd5a16e12329c0f8a"
        );
        assert_eq!(router.resolve_route_id("0").unwrap().model, "model-default");
        assert!(router.resolve_route_id("a-default").is_err());

        let identity = session_identity_from_header(Some("fixture-session")).expect("identity");
        assert_eq!(
            identity.digest,
            [
                0xd6, 0x44, 0x09, 0x83, 0xc4, 0x54, 0xc2, 0xe5, 0x99, 0x9f, 0xdb, 0x66, 0xbb, 0xe9,
                0xcf, 0x5f, 0x89, 0xa8, 0xf5, 0x84, 0x7d, 0x6c, 0x95, 0x78, 0xef, 0x14, 0xaf, 0xd2,
                0x6e, 0xee, 0x12, 0x2c,
            ]
        );
    }

    #[test]
    fn invalid_shared_policy_is_not_reimplemented_locally() {
        let mut config = config();
        config
            .model_routers
            .as_mut()
            .unwrap()
            .get_mut("virtual:code")
            .unwrap()
            .default_model = "openai/not-a-route".to_string();
        let error = compile_model_routers(&config).expect_err("shared validation should reject it");
        assert!(error.detail().contains("default_model"));
    }

    #[test]
    fn only_virtual_prefixes_are_semantic_models() {
        assert!(is_virtual_model("virtual:code"));
        assert!(!is_virtual_model("openai/gpt-4o"));
    }
}
