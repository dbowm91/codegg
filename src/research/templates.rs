//! Prompt templates for research pipeline phases.

pub const PLANNING_PROMPT: &str = r#"You are a research planner for an engineering agent harness.
Question: {question}
Mode: {mode}
Audience: {audience}
Depth: {depth}
Constraints: {constraints}

Produce a concise research plan with:
- scope
- comparison axes or investigation axes
- source classes to inspect
- exclusion criteria
- stopping conditions
- expected outputs

Return Markdown only."#;

pub const EVIDENCE_EXTRACTION_PROMPT: &str = r#"You are extracting evidence for an engineering research task.
Question: {question}
Source ID: {source_id}
Source locator: {locator}

Return JSON array only. Each item:
{{
  "text": "evidence text or precise paraphrase",
  "summary": "why this matters",
  "relevance": "low|medium|high",
  "caveats": []
}}

Do not invent evidence. Return [] if not relevant."#;

pub const CLAIM_CONSTRUCTION_PROMPT: &str = r#"You are constructing a claim graph.
Question: {question}
Evidence records: {evidence_json}

Return JSON array only. Each item:
{{
  "text": "claim",
  "claim_type": "fact|comparison|recommendation|risk|caveat|open_question|inference",
  "confidence": "low|medium|high",
  "evidence_ids": ["..."],
  "caveats": [],
  "applies_to": []
}}

Every factual or comparative claim must cite evidence_ids."#;

pub const AGENT_ANSWER_PROMPT: &str = r#"You are answering a narrow research question for another coding agent.
Question: {question}
Claims: {claims_json}
Contradictions/gaps: {contradictions_json}

Return a compact operational answer with:
- direct answer
- recommendation
- confidence
- rationale using claim IDs
- caveats
- do-not-assume list
- validation tasks
- evidence pointers

Do not include raw source text unless necessary."#;

pub const HUMAN_REPORT_PROMPT: &str = r#"You are writing a human-facing engineering research report.
Question: {question}
Mode: {mode}
Claims: {claims_json}
Contradictions/gaps: {contradictions_json}

Write a detailed Markdown report using the configured template.
Every major finding must reference claim IDs.
Separate evidence-backed claims from inferences and open questions."#;

pub const CONTRADICTION_CHECK_PROMPT: &str = r#"Given these claims, identify contradictions, stale-source risks, missing comparison axes, and questions that would change the recommendation. Return JSON only."#;

pub const VERIFICATION_PROMPT: &str = r#"Check whether each claim is supported by its cited evidence.
Return JSON with claim_id, support_status, explanation, and suggested confidence adjustment.
Do not evaluate uncited knowledge."#;
