use std::collections::HashMap;

pub struct ClosestMatch {
    lowered_terms: Vec<String>,
    original_terms: Vec<String>,
    ngram_to_term_ids: HashMap<String, Vec<usize>>,
    bag_sizes: Vec<usize>,
}

impl ClosestMatch {
    pub fn new(terms: &[&str], bag_sizes: &[usize]) -> Self {
        let original: Vec<String> = terms.iter().map(|s| s.to_string()).collect();
        let lowered: Vec<String> = terms.iter().map(|s| s.to_lowercase()).collect();
        let mut ngram_to_term_ids: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, term) in lowered.iter().enumerate() {
            let ngrams = build_ngrams(term, bag_sizes);
            for ngram in ngrams {
                ngram_to_term_ids.entry(ngram).or_default().push(idx);
            }
        }

        Self {
            lowered_terms: lowered,
            original_terms: original,
            ngram_to_term_ids,
            bag_sizes: bag_sizes.to_vec(),
        }
    }

    /// Returns the original-case term that best matches the query.
    /// Matching is performed case-insensitively via ngrams.
    pub fn closest(&self, query: &str) -> Option<&str> {
        if self.lowered_terms.is_empty() {
            return None;
        }

        let query_ngrams = build_ngrams(&query.to_lowercase(), &self.bag_sizes);

        let mut scores: HashMap<usize, usize> = HashMap::new();
        for ngram in query_ngrams {
            if let Some(term_ids) = self.ngram_to_term_ids.get(&ngram) {
                for &id in term_ids {
                    *scores.entry(id).or_insert(0) += 1;
                }
            }
        }

        let best = scores.into_iter().max_by_key(|(_, score)| *score)?;

        if best.1 == 0 {
            return None;
        }

        Some(&self.original_terms[best.0])
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.original_terms.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.original_terms.is_empty()
    }
}

fn build_ngrams(s: &str, bag_sizes: &[usize]) -> Vec<String> {
    let mut ngrams = Vec::new();
    let chars: Vec<char> = s.chars().collect();

    for &size in bag_sizes {
        if chars.len() < size {
            continue;
        }
        for i in 0..=chars.len() - size {
            let ngram: String = chars[i..i + size].iter().collect();
            if !ngram.trim().is_empty() {
                ngrams.push(ngram);
            }
        }
    }

    ngrams
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_matching() {
        let terms = vec!["hello", "world", "help"];
        let matcher = ClosestMatch::new(&terms, &[2, 3, 4]);

        assert_eq!(matcher.closest("hello"), Some("hello"));
        assert_eq!(matcher.closest("helo"), Some("hello"));
        assert_eq!(matcher.closest("wrld"), Some("world"));
    }

    #[test]
    fn test_chinese_matching() {
        let terms = vec!["星空舰", "山河图", "红莲花"];
        let matcher = ClosestMatch::new(&terms, &[2, 3, 4]);

        assert_eq!(matcher.closest("星空舰"), Some("星空舰"));
        assert_eq!(matcher.closest("星空"), Some("星空舰"));
        assert_eq!(matcher.closest("山河"), Some("山河图"));
    }

    #[test]
    fn test_empty_matcher() {
        let matcher = ClosestMatch::new(&[], &[2, 3, 4]);
        assert_eq!(matcher.closest("test"), None);
    }

    #[test]
    fn test_no_match_returns_none() {
        let terms = vec!["abc", "def"];
        let matcher = ClosestMatch::new(&terms, &[2, 3, 4]);

        assert_eq!(matcher.closest("xyz"), None);
    }

    #[test]
    fn test_case_insensitive() {
        let terms = vec!["Hello", "World"];
        let matcher = ClosestMatch::new(&terms, &[2, 3, 4]);

        assert_eq!(matcher.closest("hello"), Some("Hello"));
        assert_eq!(matcher.closest("WORLD"), Some("World"));
    }
}
