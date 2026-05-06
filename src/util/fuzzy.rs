use strsim::levenshtein;

pub fn fuzzy_match(query: &str, candidates: &[String]) -> Vec<(String, usize)> {
    let mut scored: Vec<(String, usize)> = candidates
        .iter()
        .map(|c| (c.clone(), levenshtein(query, c)))
        .collect();
    scored.sort_by_key(|(_, score)| *score);
    scored
}

pub fn fuzzy_score(query: &str, target: &str) -> usize {
    if query.is_empty() {
        return 0;
    }
    let mut query_chars = query.chars().peekable();
    let mut score = 0;
    let mut bonus = 0;
    let mut prev_matched = false;
    let mut target_iter = target.chars().enumerate().peekable();

    while let (Some(&q), Some((i, t))) = (query_chars.peek(), target_iter.peek()) {
        if q.eq_ignore_ascii_case(t) {
            score += 1;
            if *i == 0 || prev_matched {
                bonus += 1;
            }
            prev_matched = true;
            query_chars.next();
            target_iter.next();
        } else {
            prev_matched = false;
            target_iter.next();
        }
    }

    if query_chars.peek().is_some() {
        0
    } else {
        score + bonus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_exact() {
        let candidates = vec!["hello".to_string()];
        let results = fuzzy_match("hello", &candidates);
        assert_eq!(results[0].1, 0);
    }

    #[test]
    fn test_fuzzy_match_sorted_by_score() {
        let candidates = vec![
            "hello".to_string(),
            "hxllo".to_string(),
            "abcde".to_string(),
        ];
        let results = fuzzy_match("hello", &candidates);
        assert_eq!(results[0].0, "hello");
        assert_eq!(results[0].1, 0);
    }

    #[test]
    fn test_fuzzy_match_empty_query() {
        let candidates = vec!["hello".to_string()];
        let results = fuzzy_match("", &candidates);
        assert_eq!(results[0].1, 5);
    }

    #[test]
    fn test_fuzzy_score_exact() {
        let score = fuzzy_score("hello", "hello");
        assert!(score > 0);
    }

    #[test]
    fn test_fuzzy_score_partial() {
        let score = fuzzy_score("hlo", "hello");
        assert!(score > 0);
    }

    #[test]
    fn test_fuzzy_score_no_match() {
        let score = fuzzy_score("xyz", "hello");
        assert_eq!(score, 0);
    }

    #[test]
    fn test_fuzzy_score_empty_query() {
        let score = fuzzy_score("", "hello");
        assert_eq!(score, 0);
    }

    #[test]
    fn test_fuzzy_score_case_insensitive() {
        let score = fuzzy_score("HELLO", "hello");
        assert!(score > 0);
    }

    #[test]
    fn test_fuzzy_score_bonus_for_start() {
        let score_start = fuzzy_score("h", "hello");
        let score_mid = fuzzy_score("e", "hello");
        assert!(score_start > score_mid);
    }

    #[test]
    fn test_fuzzy_score_consecutive_bonus() {
        let score_consec = fuzzy_score("he", "hello");
        let score_gap = {
            let s1 = fuzzy_score("hl", "hello");
            s1
        };
        assert!(score_consec > score_gap);
    }

    #[test]
    fn test_fuzzy_score_missing_char() {
        let score = fuzzy_score("helloz", "hello");
        assert_eq!(score, 0);
    }
}
