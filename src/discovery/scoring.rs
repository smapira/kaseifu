use std::collections::HashSet;

pub const W_FILENAME: f64 = 0.60;
pub const W_TOKEN: f64 = 0.25;
pub const W_EXT: f64 = 0.15;

pub const DIR_W_MAX: f64 = 0.85;
pub const DIR_W_MEAN_TOP3: f64 = 0.15;

#[derive(Debug, Clone, Default)]
pub struct NormalizedName {
    pub stem: String,
    pub normalized: String,
    pub tokens: Vec<String>,
}

fn fold_fullwidth(c: char) -> char {
    let n = c as u32;
    if (0xFF01..=0xFF5E).contains(&n) {
        char::from_u32(n - 0xFEE0).unwrap_or(c)
    } else if n == 0x3000 {
        ' '
    } else {
        c
    }
}

fn is_separator(c: char) -> bool {
    matches!(
        c,
        '-' | '_'
            | ' '
            | '\t'
            | '.'
            | '/'
            | '\\'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '+'
            | ','
            | ';'
            | ':'
            | '\u{3000}'
    )
}

pub fn split_stem(file_name: &str) -> (String, Option<String>) {
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    match name.rfind('.') {
        Some(idx) if idx > 0 => (name[..idx].to_string(), Some(name[idx + 1..].to_string())),
        _ => (name.to_string(), None),
    }
}

pub fn normalize_name(file_name: &str) -> NormalizedName {
    let (stem, _) = split_stem(file_name);
    let mut lowered = String::with_capacity(stem.len());
    for c in stem.chars() {
        lowered.push(fold_fullwidth(c));
    }
    let lowered = lowered.to_lowercase();
    let mut unified = String::with_capacity(lowered.len());
    let mut prev_sep = false;
    for c in lowered.chars() {
        if is_separator(c) {
            if !prev_sep {
                unified.push(' ');
            }
            prev_sep = true;
        } else {
            unified.push(c);
            prev_sep = false;
        }
    }
    let normalized = unified.split_whitespace().collect::<Vec<_>>().join(" ");
    let tokens = if normalized.is_empty() {
        Vec::new()
    } else {
        normalized.split(' ').map(|s| s.to_string()).collect()
    };
    NormalizedName {
        stem,
        normalized,
        tokens,
    }
}

pub fn extension_of(file_name: &str) -> Option<String> {
    let (_, ext) = split_stem(file_name);
    ext.map(|e| e.to_lowercase()).filter(|e| !e.is_empty())
}

pub fn extension_match(target_ext: Option<&str>, candidate_path: &str) -> f64 {
    let cand = extension_of(candidate_path);
    match (target_ext.map(|s| s.to_lowercase()), cand) {
        (None, None) => 1.0,
        (Some(_), None) | (None, Some(_)) => 0.0,
        (Some(a), Some(b)) => {
            if a.eq_ignore_ascii_case(&b) {
                1.0
            } else {
                0.0
            }
        }
    }
}

pub fn token_jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let sa: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let sb: HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    if union == 0.0 { 0.0 } else { inter / union }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost).min((curr[j] + 1).min(prev[j + 1] + 1));
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

pub fn filename_similarity(norm_target: &str, norm_candidate: &str) -> f64 {
    if norm_target == norm_candidate {
        return 1.0;
    }
    if norm_target.is_empty() || norm_candidate.is_empty() {
        return 0.0;
    }
    let max_len = norm_target
        .chars()
        .count()
        .max(norm_candidate.chars().count()) as f64;
    if max_len == 0.0 {
        return 1.0;
    }
    let dist = levenshtein(norm_target, norm_candidate) as f64;
    (1.0 - dist / max_len).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Default)]
pub struct FileScore {
    pub filename_similarity: f64,
    pub token_similarity: f64,
    pub extension_match: f64,
    pub score: f64,
    pub matched_terms: Vec<String>,
}

pub fn score_file(
    target_file_name: &str,
    target_ext: Option<&str>,
    candidate_path: &str,
    search_terms: &[String],
) -> FileScore {
    let target_norm = normalize_name(target_file_name);
    let cand_file_name = candidate_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(candidate_path);
    let cand_norm = normalize_name(cand_file_name);
    let filename_similarity = filename_similarity(&target_norm.normalized, &cand_norm.normalized);
    let token_similarity = token_jaccard(&target_norm.tokens, &cand_norm.tokens);
    let extension_match = extension_match(target_ext, candidate_path);
    let score =
        (W_FILENAME * filename_similarity + W_TOKEN * token_similarity + W_EXT * extension_match)
            .clamp(0.0, 1.0);
    let matched_terms = matched_search_terms(search_terms, &cand_norm);
    FileScore {
        filename_similarity,
        token_similarity,
        extension_match,
        score,
        matched_terms,
    }
}

fn matched_search_terms(terms: &[String], cand: &NormalizedName) -> Vec<String> {
    let mut out = Vec::new();
    for t in terms {
        let nt = normalize_name(t);
        if nt.normalized.is_empty() {
            continue;
        }
        let hit = cand.tokens.iter().any(|tok| tok == &nt.normalized)
            || cand.normalized.contains(&nt.normalized);
        if hit {
            out.push(t.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Weight of the age adjustment on directory scores (design hypothesis,
/// validated against fixtures; filename similarity still dominates).
pub const AGE_WEIGHT: f64 = 0.10;
/// Median evidence age (days) at which the age bonus saturates.
pub const AGE_REF_DAYS: f64 = 365.0;

/// Median of unix timestamps, if any.
pub fn median_timestamp(mut ts: Vec<i64>) -> Option<i64> {
    if ts.is_empty() {
        return None;
    }
    ts.sort_unstable();
    let mid = ts.len() / 2;
    if ts.len() % 2 == 1 {
        Some(ts[mid])
    } else {
        Some((ts[mid - 1] + ts[mid]) / 2)
    }
}

/// Age bonus in 0..=1 from a median evidence age in days: linear up to
/// [`AGE_REF_DAYS`], clamped on both ends (future dates yield 0).
pub fn age_bonus(median_age_days: f64) -> f64 {
    (median_age_days / AGE_REF_DAYS).clamp(0.0, 1.0)
}

/// Apply the age adjustment to a base directory score.
///
/// Returns `(adjusted_score, bonus)`. Missing dates yield no adjustment.
pub fn adjust_for_age(base: f64, median_age_days: Option<f64>) -> (f64, f64) {
    let bonus = median_age_days.map(age_bonus).unwrap_or(0.0);
    ((base + AGE_WEIGHT * bonus).clamp(0.0, 1.0), bonus)
}

pub fn aggregate_directory_score(mut scores: Vec<f64>) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let max = scores[0];
    let top3: Vec<f64> = scores.into_iter().take(3).collect();
    let mean = top3.iter().sum::<f64>() / top3.len() as f64;
    (DIR_W_MAX * max + DIR_W_MEAN_TOP3 * mean).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_bonus_curve() {
        assert_eq!(age_bonus(0.0), 0.0);
        assert_eq!(age_bonus(-5.0), 0.0);
        assert!((age_bonus(182.5) - 0.5).abs() < 1e-9);
        assert_eq!(age_bonus(365.0), 1.0);
        assert_eq!(age_bonus(10_000.0), 1.0);
    }

    #[test]
    fn adjust_for_age_adds_bounded_bonus() {
        let (score, bonus) = adjust_for_age(0.8, Some(365.0));
        assert!((bonus - 1.0).abs() < 1e-9);
        assert!((score - 0.9).abs() < 1e-9);
        // Capped at 1.0.
        assert_eq!(adjust_for_age(1.0, Some(365.0)).0, 1.0);
        // Missing dates: neutral.
        assert_eq!(adjust_for_age(0.8, None), (0.8, 0.0));
    }

    #[test]
    fn median_timestamp_middle() {
        assert_eq!(median_timestamp(vec![]), None);
        assert_eq!(median_timestamp(vec![7]), Some(7));
        assert_eq!(median_timestamp(vec![9, 1, 5]), Some(5));
        assert_eq!(median_timestamp(vec![1, 2, 3, 100]), Some(2));
    }
}
