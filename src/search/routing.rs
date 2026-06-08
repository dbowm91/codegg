//! Query-shape classification.
//!
//! The web search tool uses these predicates to decide which providers
//! to invoke. The general provider chain (keys → DDG → Mojeek) is
//! always tried first; if it returns thin results and the query shape
//! matches a domain predicate, the matching domain provider is
//! queried as well and its results are merged.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryKind {
    General,
    Encyclopedic,
    Academic,
    Biomedical,
    News,
    TechDiscourse,
    Code,
}

impl QueryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueryKind::General => "general",
            QueryKind::Encyclopedic => "encyclopedic",
            QueryKind::Academic => "academic",
            QueryKind::Biomedical => "biomedical",
            QueryKind::News => "news",
            QueryKind::TechDiscourse => "tech_discourse",
            QueryKind::Code => "code",
        }
    }
}

const ACADEMIC_HINTS: &[&str] = &[
    "arxiv",
    "paper",
    "preprint",
    "study",
    "research",
    "literature",
    "academic",
    "scholar",
    "doi",
    "citation",
    "journal",
];

const BIOMEDICAL_HINTS: &[&str] = &[
    "pubmed",
    "biomedical",
    "clinical",
    "patient",
    "drug",
    "disease",
    "treatment",
    "diagnosis",
    "trial",
    "fda",
    "pharma",
    "genomic",
    "protein",
    "side effect",
    "vaccine",
    "antibody",
    "symptom",
    "pathology",
    "oncology",
    "cardiology",
    "neurology",
];

const NEWS_HINTS: &[&str] = &[
    "news",
    "breaking",
    "latest",
    "today",
    "this week",
    "headline",
    "announced",
    "released",
    "report says",
    "according to",
];

const TECH_DISCOURSE_HINTS: &[&str] = &[
    "hacker news",
    "hn",
    "discussion",
    "reddit",
    "lobsters",
    "show hn",
    "ask hn",
    "launch hn",
    "community",
];

const CODE_HINTS: &[&str] = &[
    "github",
    "repo",
    "repository",
    "source code",
    "implementation of",
    "library on github",
    "crate on",
];

const ENTITY_HINTS: &[&str] = &[
    "what is",
    "who is",
    "define",
    "definition of",
    "meaning of",
    "history of",
    "biography of",
    "explain",
];

/// Heuristic, lowercase, substring-based. Cheap and obvious on purpose.
fn contains_any_ci(haystack: &str, needles: &[&str]) -> bool {
    let lower = haystack.to_lowercase();
    needles.iter().any(|n| lower.contains(n))
}

/// Classify a query into a single (best-guess) kind.
pub fn classify_query(query: &str) -> QueryKind {
    if contains_any_ci(query, BIOMEDICAL_HINTS) {
        return QueryKind::Biomedical;
    }
    if contains_any_ci(query, ACADEMIC_HINTS) {
        return QueryKind::Academic;
    }
    if contains_any_ci(query, CODE_HINTS) {
        return QueryKind::Code;
    }
    if contains_any_ci(query, NEWS_HINTS) {
        return QueryKind::News;
    }
    if contains_any_ci(query, TECH_DISCOURSE_HINTS) {
        return QueryKind::TechDiscourse;
    }
    if contains_any_ci(query, ENTITY_HINTS) {
        return QueryKind::Encyclopedic;
    }
    QueryKind::General
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_biomedical() {
        assert_eq!(
            classify_query("side effects of metformin"),
            QueryKind::Biomedical
        );
    }

    #[test]
    fn classifies_academic() {
        assert_eq!(
            classify_query("transformer paper attention is all you need"),
            QueryKind::Academic
        );
    }

    #[test]
    fn classifies_news() {
        assert_eq!(
            classify_query("breaking news: rust 1.85 release"),
            QueryKind::News
        );
    }

    #[test]
    fn classifies_entity() {
        assert_eq!(
            classify_query("what is a tokio runtime"),
            QueryKind::Encyclopedic
        );
    }

    #[test]
    fn classifies_general() {
        assert_eq!(classify_query("best pizza near me"), QueryKind::General);
    }
}
