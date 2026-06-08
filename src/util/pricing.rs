use std::collections::HashMap;

/// Pricing data for a single model.
///
/// All prices are in USD per million tokens.
#[derive(Debug, Clone)]
pub struct ModelPricing {
    /// Price per million input tokens
    pub input_per_m: f64,
    /// Price per million output tokens
    pub output_per_m: f64,
    /// Discount factor for cached tokens (0.0 = no discount, 1.0 = free)
    pub cached_discount: f64,
}

/// Service for calculating LLM API costs based on token usage.
///
/// Provides pricing for major providers: OpenAI, Anthropic, Google, and MiniMax.
/// Pricing is looked up by provider/model key (e.g., "openai/gpt-4o").
pub struct PricingService {
    rates: HashMap<String, ModelPricing>,
}

impl PricingService {
    /// Creates a new PricingService with default pricing rates for known models.
    ///
    /// Covers OpenAI (GPT-4, GPT-3.5, o1, o3), Anthropic (Claude), Google (Gemini), and MiniMax.
    pub fn new() -> Self {
        let mut rates = HashMap::new();

        rates.insert(
            "openai/gpt-4o".to_string(),
            ModelPricing {
                input_per_m: 2.50,
                output_per_m: 10.0,
                cached_discount: 0.5,
            },
        );
        rates.insert(
            "openai/gpt-4o-mini".to_string(),
            ModelPricing {
                input_per_m: 0.15,
                output_per_m: 0.60,
                cached_discount: 0.5,
            },
        );
        rates.insert(
            "openai/gpt-4-turbo".to_string(),
            ModelPricing {
                input_per_m: 10.0,
                output_per_m: 30.0,
                cached_discount: 0.5,
            },
        );
        rates.insert(
            "openai/gpt-4".to_string(),
            ModelPricing {
                input_per_m: 30.0,
                output_per_m: 60.0,
                cached_discount: 0.5,
            },
        );
        rates.insert(
            "openai/gpt-4-32k".to_string(),
            ModelPricing {
                input_per_m: 60.0,
                output_per_m: 120.0,
                cached_discount: 0.5,
            },
        );
        rates.insert(
            "openai/gpt-3.5-turbo".to_string(),
            ModelPricing {
                input_per_m: 0.50,
                output_per_m: 1.50,
                cached_discount: 0.5,
            },
        );

        rates.insert(
            "anthropic/claude-opus".to_string(),
            ModelPricing {
                input_per_m: 15.0,
                output_per_m: 75.0,
                cached_discount: 0.9,
            },
        );
        rates.insert(
            "anthropic/claude-sonnet".to_string(),
            ModelPricing {
                input_per_m: 3.0,
                output_per_m: 15.0,
                cached_discount: 0.9,
            },
        );
        rates.insert(
            "anthropic/claude-haiku".to_string(),
            ModelPricing {
                input_per_m: 0.25,
                output_per_m: 1.25,
                cached_discount: 0.9,
            },
        );
        rates.insert(
            "anthropic/claude-3-5-sonnet".to_string(),
            ModelPricing {
                input_per_m: 3.0,
                output_per_m: 15.0,
                cached_discount: 0.9,
            },
        );
        rates.insert(
            "anthropic/claude-3-5-haiku".to_string(),
            ModelPricing {
                input_per_m: 0.25,
                output_per_m: 1.25,
                cached_discount: 0.9,
            },
        );

        rates.insert(
            "google/gemini-2.0-flash".to_string(),
            ModelPricing {
                input_per_m: 0.0,
                output_per_m: 0.0,
                cached_discount: 0.0,
            },
        );
        rates.insert(
            "google/gemini-1.5-pro".to_string(),
            ModelPricing {
                input_per_m: 1.25,
                output_per_m: 5.0,
                cached_discount: 0.5,
            },
        );
        rates.insert(
            "google/gemini-1.5-flash".to_string(),
            ModelPricing {
                input_per_m: 0.075,
                output_per_m: 0.30,
                cached_discount: 0.5,
            },
        );
        rates.insert(
            "google/gemini-pro".to_string(),
            ModelPricing {
                input_per_m: 0.125,
                output_per_m: 0.50,
                cached_discount: 0.5,
            },
        );

        rates.insert(
            "minimax/minimax".to_string(),
            ModelPricing {
                input_per_m: 0.0,
                output_per_m: 0.0,
                cached_discount: 0.0,
            },
        );

        rates.insert(
            "openai/o1".to_string(),
            ModelPricing {
                input_per_m: 15.0,
                output_per_m: 60.0,
                cached_discount: 0.0,
            },
        );
        rates.insert(
            "openai/o1-mini".to_string(),
            ModelPricing {
                input_per_m: 0.55,
                output_per_m: 3.50,
                cached_discount: 0.0,
            },
        );
        rates.insert(
            "openai/o1-preview".to_string(),
            ModelPricing {
                input_per_m: 15.0,
                output_per_m: 60.0,
                cached_discount: 0.0,
            },
        );
        rates.insert(
            "openai/o3".to_string(),
            ModelPricing {
                input_per_m: 10.0,
                output_per_m: 40.0,
                cached_discount: 0.0,
            },
        );
        rates.insert(
            "openai/o3-mini".to_string(),
            ModelPricing {
                input_per_m: 1.10,
                output_per_m: 4.40,
                cached_discount: 0.0,
            },
        );

        Self { rates }
    }

    /// Calculates the cost in USD for an API call.
    ///
    /// # Arguments
    ///
    /// * `provider` - Provider name (e.g., "openai", "anthropic")
    /// * `model` - Model name (e.g., "gpt-4o", "claude-sonnet")
    /// * `input_tokens` - Total input tokens (including cached)
    /// * `output_tokens` - Output tokens consumed
    /// * `cached_tokens` - Portion of input tokens that were cached
    ///
    /// # Returns
    ///
    /// Cost in USD, or 0.0 if the model is not found in the pricing table.
    ///
    /// # Pricing Formula
    ///
    /// ```text
    /// input_cost = ((non_cached_input / 1_000_000) * input_per_m)
    ///            + ((cached_input / 1_000_000) * input_per_m * cached_discount)
    /// output_cost = (output_tokens / 1_000_000) * output_per_m
    /// total = input_cost + output_cost
    /// ```
    ///
    /// Where `non_cached_input = input_tokens - cached_tokens`.
    pub fn calculate_cost(
        &self,
        provider: &str,
        model: &str,
        input_tokens: i64,
        output_tokens: i64,
        cached_tokens: i64,
    ) -> f64 {
        let key = format!("{}/{}", provider.to_lowercase(), model.to_lowercase());

        let pricing = self.rates.get(&key).or_else(|| {
            self.rates
                .iter()
                .find(|(k, _)| {
                    let k_lower = k.to_lowercase();
                    let key_lower = key.to_lowercase();
                    k_lower.contains(&key_lower) || key_lower.contains(&k_lower)
                })
                .map(|(_, v)| v)
        });

        let Some(pricing) = pricing else {
            return 0.0;
        };

        let non_cached_input = (input_tokens - cached_tokens).max(0) as f64;
        let cached_input = cached_tokens as f64;

        let input_cost = (non_cached_input / 1_000_000.0) * pricing.input_per_m
            + (cached_input / 1_000_000.0) * pricing.input_per_m * pricing.cached_discount;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * pricing.output_per_m;

        input_cost + output_cost
    }
}

impl Default for PricingService {
    fn default() -> Self {
        Self::new()
    }
}
