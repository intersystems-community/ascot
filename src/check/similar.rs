//! "Did you mean" ranking: the closest names to a misspelled one, by Jaro-Winkler similarity.

// The `n` closest candidates, case-insensitively. Jaro-Winkler rewards a shared prefix, which
// is what a typo like `ISERROR` for `ISERR` is; plain edit distance wouldn't.
pub(crate) fn top_similar<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
    n: usize,
) -> Vec<String> {
    let target = name.to_lowercase();
    let mut scored: Vec<(f64, &str)> = candidates
        .filter(|candidate| !candidate.eq_ignore_ascii_case(name))
        .map(|candidate| (jaro_winkler(&target, &candidate.to_lowercase()), candidate))
        .collect();
    scored.sort_by(|(s1, n1), (s2, n2)| s2.total_cmp(s1).then_with(|| n1.cmp(n2)));
    // Case variants tie, so the sort, not the candidates' arrival order, decides which survives.
    scored.dedup_by_key(|(_, name)| name.to_lowercase());
    scored
        .into_iter()
        .take(n)
        .map(|(_, name)| name.to_string())
        .collect()
}

// 1.0 = identical, 0.0 = nothing in common. See Winkler (1990); the standard algorithm (a Jaro
// score, boosted for up to 4 shared leading characters).
fn jaro_winkler(a: &str, b: &str) -> f64 {
    let jaro = jaro(a, b);
    let prefix_len = a
        .chars()
        .zip(b.chars())
        .take(4)
        .take_while(|(x, y)| x == y)
        .count();
    jaro + prefix_len as f64 * 0.1 * (1.0 - jaro)
}

fn jaro(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return if a.is_empty() && b.is_empty() {
            1.0
        } else {
            0.0
        };
    }
    let match_distance = a.len().max(b.len()) / 2 - if a.len().max(b.len()) < 2 { 0 } else { 1 };
    let mut a_matched = vec![false; a.len()];
    let mut b_matched = vec![false; b.len()];
    let mut matches = 0usize;
    for (i, &ca) in a.iter().enumerate() {
        let start = i.saturating_sub(match_distance);
        let end = (i + match_distance + 1).min(b.len());
        for (j, matched) in b_matched.iter_mut().enumerate().take(end).skip(start) {
            if !*matched && ca == b[j] {
                a_matched[i] = true;
                *matched = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let mut transpositions = 0usize;
    let mut b_iter = b
        .iter()
        .zip(b_matched.iter())
        .filter(|(_, m)| **m)
        .map(|(c, _)| c);
    for (&ca, _) in a.iter().zip(a_matched.iter()).filter(|(_, m)| **m) {
        if let Some(&cb) = b_iter.next()
            && ca != cb
        {
            transpositions += 1;
        }
    }
    let m = matches as f64;
    (m / a.len() as f64 + m / b.len() as f64 + (m - (transpositions / 2) as f64) / m) / 3.0
}
