//! Descriptor-declared model metadata, applied through the loop's existing
//! `ModelProfileResolver` seam rather than a parallel lookup.

use crate::model_profile::resolve::ModelProfileResolver;
use crate::model_profile::types::ResolvedModelProfile;
use codegg_config::schema::{CompactionConfig, Config};
use codegg_providers::inline::InlineModel;

/// The facts a descriptor declares about one model. Absent fields stay
/// absent, so the built-in resolver's value stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineModelProfile {
    pub model: String,
    pub context_window: Option<usize>,
    pub max_output_tokens: Option<usize>,
    pub supports_tools: Option<bool>,
}

impl InlineModelProfile {
    pub fn from_inline_model(model: &InlineModel) -> Self {
        Self {
            model: model.id.clone(),
            context_window: model.context_window,
            max_output_tokens: model.max_output_tokens,
            supports_tools: model.supports_tools,
        }
    }

    pub fn has_metadata(&self) -> bool {
        self.context_window.is_some()
            || self.max_output_tokens.is_some()
            || self.supports_tools.is_some()
    }

    /// The profile the loop resolves once [`Self::apply_to_config`] has
    /// installed these facts.
    pub fn resolve(&self, config: &Config) -> ResolvedModelProfile {
        let mut profile = ModelProfileResolver::new(config).resolve(&self.model);
        if let Some(context_window) = self.context_window {
            profile.context_window = Some(context_window);
        }
        if let Some(max_output_tokens) = self.max_output_tokens {
            profile.max_output_tokens = Some(max_output_tokens);
        }
        profile
    }

    /// Install the declared bounds into the per-session config the loop
    /// already reads. `compaction` is the policy's first source for the
    /// context limit and the reserved output budget.
    pub fn apply_to_config(&self, config: &mut Config) {
        if self.context_window.is_none() && self.max_output_tokens.is_none() {
            return;
        }
        let profiles = config.model_profile.get_or_insert_with(Default::default);
        let entry = profiles.entry(self.model.clone()).or_default();
        if let Some(context_window) = self.context_window {
            entry.context_window = Some(context_window);
        }
        if let Some(max_output_tokens) = self.max_output_tokens {
            entry.max_output_tokens = Some(max_output_tokens);
        }
        let compaction = config
            .compaction
            .get_or_insert_with(CompactionConfig::default);
        if let Some(context_window) = self.context_window {
            compaction.max_tokens = Some(context_window);
        }
        if let Some(max_output_tokens) = self.max_output_tokens {
            compaction.reserved = Some(max_output_tokens);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_providers::inline::InlineModel;

    fn inline_model(
        id: &str,
        context_window: Option<usize>,
        max_output_tokens: Option<usize>,
    ) -> InlineModel {
        InlineModel {
            id: id.to_string(),
            context_window,
            max_output_tokens,
            supports_tools: None,
        }
    }

    #[test]
    fn declared_metadata_reaches_the_resolved_profile_and_execution_bounds() {
        let mut model = inline_model("descriptor-model", Some(64_000), Some(4_096));
        model.supports_tools = Some(true);
        let inline = InlineModelProfile::from_inline_model(&model);
        let mut config = Config::default();
        inline.apply_to_config(&mut config);

        let profile = inline.resolve(&config);
        assert_eq!(profile.context_window, Some(64_000));
        assert_eq!(profile.max_output_tokens, Some(4_096));
        assert_eq!(inline.supports_tools, Some(true));
        assert_eq!(
            config
                .model_profile
                .as_ref()
                .and_then(|profiles| profiles.get("descriptor-model")),
            Some(&codegg_config::schema::ModelProfileConfig {
                context_window: Some(64_000),
                max_output_tokens: Some(4_096),
                ..Default::default()
            })
        );
        let compaction = config.compaction.expect("compaction bounds were installed");
        assert_eq!(compaction.max_tokens, Some(64_000));
        assert_eq!(compaction.reserved, Some(4_096));
    }

    #[test]
    fn absent_metadata_falls_back_to_the_existing_resolver_untouched() {
        let model = inline_model("unknown/descriptor-model", None, None);
        let inline = InlineModelProfile::from_inline_model(&model);
        let mut config = Config::default();
        inline.apply_to_config(&mut config);

        assert!(!inline.has_metadata());
        assert_eq!(config.model_profile, None);
        assert_eq!(config.compaction, None);
        assert_eq!(
            inline.resolve(&config),
            ModelProfileResolver::new(&config).resolve("unknown/descriptor-model")
        );
    }

    #[test]
    fn absent_field_keeps_the_resolver_value_when_metadata_is_partial() {
        let model = inline_model("partial-model", Some(32_000), None);
        let inline = InlineModelProfile::from_inline_model(&model);
        let config = Config::default();
        let base = ModelProfileResolver::new(&config).resolve("partial-model");
        let resolved = inline.resolve(&config);

        assert_eq!(resolved.context_window, Some(32_000));
        assert_eq!(resolved.max_output_tokens, base.max_output_tokens);
    }
}
