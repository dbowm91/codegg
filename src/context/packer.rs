use super::block::{CacheClass, ContextBlock, ContextBlockId, ContextBlockKind};

#[derive(Debug, Clone)]
pub struct ContextPackBudget {
    pub max_tokens: usize,
    pub reserved_output_tokens: usize,
    pub emergency_margin_tokens: usize,
}

impl ContextPackBudget {
    pub fn available_for_context(&self) -> usize {
        self.max_tokens
            .saturating_sub(self.reserved_output_tokens)
            .saturating_sub(self.emergency_margin_tokens)
    }
}

#[derive(Debug, Clone)]
pub struct ContextPackResult {
    pub blocks: Vec<ContextBlock>,
    pub estimated_tokens: usize,
    pub omitted_blocks: Vec<OmittedContextBlock>,
    pub stable_prefix_tokens: usize,
    pub volatile_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct OmittedContextBlock {
    pub id: ContextBlockId,
    pub kind: ContextBlockKind,
    pub estimated_tokens: usize,
    pub reason: OmissionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OmissionReason {
    OverBudget,
    LowerPriority,
    VolatileOverflow,
    ReplacedByHandle,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SortKey {
    tier: CacheClass,
    priority_rev: std::cmp::Reverse<u32>,
    id: ContextBlockId,
}

impl SortKey {
    fn for_block(block: &ContextBlock) -> Self {
        Self {
            tier: block.kind.tier(),
            priority_rev: std::cmp::Reverse(block.priority),
            id: block.id.clone(),
        }
    }
}

pub fn pack(blocks: Vec<ContextBlock>, budget: &ContextPackBudget) -> ContextPackResult {
    let mut sorted = blocks;
    sorted.sort_by_key(SortKey::for_block);

    let mut included = Vec::new();
    let mut omitted = Vec::new();
    let mut total_tokens: usize = 0;
    let available = budget.available_for_context();

    for block in sorted {
        let tier = block.kind.tier();

        if block.required {
            total_tokens += block.estimated_tokens;
            included.push(block);
            continue;
        }

        if tier == CacheClass::NeverCache && !block.required {
            omitted.push(OmittedContextBlock {
                id: block.id.clone(),
                kind: block.kind,
                estimated_tokens: block.estimated_tokens,
                reason: OmissionReason::OverBudget,
            });
            continue;
        }

        if tier == CacheClass::Volatile
            && block.priority < 10
            && !block.required
            && total_tokens + block.estimated_tokens > available
        {
            omitted.push(OmittedContextBlock {
                id: block.id.clone(),
                kind: block.kind,
                estimated_tokens: block.estimated_tokens,
                reason: OmissionReason::LowerPriority,
            });
            continue;
        }

        if total_tokens + block.estimated_tokens > available {
            omitted.push(OmittedContextBlock {
                id: block.id.clone(),
                kind: block.kind,
                estimated_tokens: block.estimated_tokens,
                reason: OmissionReason::OverBudget,
            });
            continue;
        }

        total_tokens += block.estimated_tokens;
        included.push(block);
    }

    let stable_prefix_tokens: usize = included
        .iter()
        .filter(|b| b.kind.tier() == CacheClass::StablePrefix)
        .map(|b| b.estimated_tokens)
        .sum();

    let volatile_tokens: usize = included
        .iter()
        .filter(|b| b.kind.tier() == CacheClass::Volatile)
        .map(|b| b.estimated_tokens)
        .sum();

    ContextPackResult {
        blocks: included,
        estimated_tokens: total_tokens,
        omitted_blocks: omitted,
        stable_prefix_tokens,
        volatile_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::super::block::Lossiness;
    use super::*;

    fn block(
        kind: ContextBlockKind,
        source: &str,
        tokens: usize,
        priority: u32,
        required: bool,
    ) -> ContextBlock {
        let text = "x".repeat(tokens * 4);
        let mut b = ContextBlock::new(kind, source, text, priority, required, Lossiness::Lossless);
        b.estimated_tokens = tokens;
        b
    }

    fn budget(max: usize) -> ContextPackBudget {
        ContextPackBudget {
            max_tokens: max,
            reserved_output_tokens: 0,
            emergency_margin_tokens: 0,
        }
    }

    #[test]
    fn stable_before_volatile() {
        let volatile = block(ContextBlockKind::UserMessage, "u1", 100, 50, false);
        let stable = block(ContextBlockKind::SystemPrompt, "s1", 100, 50, false);
        let result = pack(vec![volatile, stable.clone()], &budget(1000));
        assert_eq!(result.blocks.len(), 2);
        assert_eq!(result.blocks[0].kind, ContextBlockKind::SystemPrompt);
        assert_eq!(result.blocks[1].kind, ContextBlockKind::UserMessage);
    }

    #[test]
    fn low_priority_volatile_omitted_first() {
        let high_prio = block(ContextBlockKind::UserMessage, "high", 100, 50, false);
        let low_prio = block(ContextBlockKind::AssistantMessage, "low", 100, 5, false);
        let budget = budget(150);
        let result = pack(vec![high_prio, low_prio], &budget);
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.blocks[0].source, "high");
        assert_eq!(result.omitted_blocks.len(), 1);
        assert_eq!(
            result.omitted_blocks[0].kind,
            ContextBlockKind::AssistantMessage
        );
        assert_eq!(
            result.omitted_blocks[0].reason,
            OmissionReason::LowerPriority
        );
    }

    #[test]
    fn omitted_blocks_have_reasons() {
        let b = block(ContextBlockKind::ToolResult, "t1", 200, 50, false);
        let result = pack(vec![b], &budget(100));
        assert_eq!(result.omitted_blocks.len(), 1);
        assert_eq!(result.omitted_blocks[0].reason, OmissionReason::OverBudget);
    }

    #[test]
    fn transcript_order_preserved() {
        let a = block(ContextBlockKind::UserMessage, "msg1", 50, 50, false);
        let b = block(ContextBlockKind::AssistantMessage, "msg2", 50, 50, false);
        let c = block(ContextBlockKind::ToolResult, "res1", 50, 50, false);
        let result = pack(vec![c.clone(), a.clone(), b.clone()], &budget(1000));
        let kinds: Vec<_> = result.blocks.iter().map(|b| b.kind).collect();
        assert!(kinds.contains(&ContextBlockKind::UserMessage));
        assert!(kinds.contains(&ContextBlockKind::AssistantMessage));
        assert!(kinds.contains(&ContextBlockKind::ToolResult));
    }

    #[test]
    fn budget_accounting() {
        let a = block(ContextBlockKind::SystemPrompt, "s1", 100, 100, true);
        let b = block(ContextBlockKind::ToolDefinitions, "t1", 200, 80, false);
        let c = block(ContextBlockKind::UserMessage, "u1", 150, 50, false);
        let result = pack(vec![a, b, c], &budget(400));
        assert_eq!(result.estimated_tokens, 300);
        assert_eq!(result.omitted_blocks.len(), 1);
        assert_eq!(result.omitted_blocks[0].kind, ContextBlockKind::UserMessage);
    }

    #[test]
    fn required_blocks_always_included() {
        let required = block(ContextBlockKind::ControlInstruction, "ctrl", 300, 1, true);
        let result = pack(vec![required], &budget(100));
        assert_eq!(result.blocks.len(), 1);
        assert!(result.blocks[0].required);
    }

    #[test]
    fn never_cache_non_required_omitted() {
        let ctrl = block(ContextBlockKind::ControlInstruction, "ctrl", 50, 100, false);
        let result = pack(vec![ctrl], &budget(10000));
        assert!(result.blocks.is_empty());
        assert_eq!(result.omitted_blocks.len(), 1);
        assert_eq!(result.omitted_blocks[0].reason, OmissionReason::OverBudget);
    }

    #[test]
    fn stable_prefix_tokens_accounting() {
        let sys = block(ContextBlockKind::SystemPrompt, "sys", 100, 100, false);
        let model = block(ContextBlockKind::ModelProfile, "model", 200, 90, false);
        let user = block(ContextBlockKind::UserMessage, "user", 50, 50, false);
        let result = pack(vec![user, sys, model], &budget(10000));
        assert_eq!(result.stable_prefix_tokens, 300);
        assert_eq!(result.volatile_tokens, 50);
    }
}
