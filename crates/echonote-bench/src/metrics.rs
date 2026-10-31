/// Levenshtein distance between two token sequences.
pub fn edit_distance<T: PartialEq>(reference: &[T], hypothesis: &[T]) -> usize {
    let mut row: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, reference_token) in reference.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, hypothesis_token) in hypothesis.iter().enumerate() {
            let substitution = diagonal + usize::from(reference_token != hypothesis_token);
            diagonal = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(diagonal + 1);
        }
    }
    row[hypothesis.len()]
}

/// Length of the longest common subsequence.
pub fn lcs_len<T: PartialEq>(left: &[T], right: &[T]) -> usize {
    let mut row = vec![0usize; right.len() + 1];
    for left_token in left {
        let mut diagonal = 0;
        for (j, right_token) in right.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if left_token == right_token {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[right.len()]
}

/// Nearest-rank percentile of an unsorted sample; `None` when empty.
pub fn percentile(values: &[f64], quantile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = ((quantile * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    Some(sorted[rank - 1])
}

pub fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_counts_substitutions_insertions_deletions() {
        assert_eq!(edit_distance(&['a', 'b', 'c'], &['a', 'x', 'c', 'd']), 2);
        assert_eq!(edit_distance::<char>(&[], &['a']), 1);
        assert_eq!(edit_distance(&['a'], &[]), 1);
    }

    #[test]
    fn lcs_matches_subsequence() {
        assert_eq!(lcs_len(&['a', 'b', 'c', 'd'], &['b', 'd']), 2);
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let values = [5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(percentile(&values, 0.5), Some(3.0));
        assert_eq!(percentile(&values, 0.95), Some(5.0));
        assert_eq!(percentile(&[], 0.5), None);
    }
}
