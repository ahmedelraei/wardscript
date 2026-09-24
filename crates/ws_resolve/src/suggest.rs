/// The closest candidate to `name`, if it's close enough to be a plausible typo.
pub fn did_you_mean<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let max = (name.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .filter(|c| *c != name)
        .map(|c| (distance(name, c), c))
        .filter(|&(d, _)| d <= max)
        .min_by_key(|&(d, c)| (d, c))
        .map(|(_, c)| c)
}

/// Levenshtein distance; letters differing only in case count as equal.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, &cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(!ca.eq_ignore_ascii_case(&cb));
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::did_you_mean;

    #[test]
    fn suggests_close_names_only() {
        let names = ["ticket", "triage", "summary"];
        assert_eq!(did_you_mean("tiket", names), Some("ticket"));
        assert_eq!(did_you_mean("Ticket", names), Some("ticket"));
        assert_eq!(did_you_mean("xyz", names), None);
    }
}
