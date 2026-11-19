//! Word error rate for the accuracy benchmark (SPEC 13.1). Pure text, no audio.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wer {
    pub ref_words: usize,
    pub substitutions: usize,
    pub deletions: usize,
    pub insertions: usize,
}

impl Wer {
    pub fn errors(&self) -> usize {
        self.substitutions + self.deletions + self.insertions
    }
    pub fn rate(&self) -> f64 {
        if self.ref_words == 0 {
            return if self.errors() == 0 { 0.0 } else { 1.0 };
        }
        self.errors() as f64 / self.ref_words as f64
    }
    pub fn add(&mut self, o: &Wer) {
        self.ref_words += o.ref_words;
        self.substitutions += o.substitutions;
        self.deletions += o.deletions;
        self.insertions += o.insertions;
    }
}

/// Lowercase, keep letters/digits/apostrophes, drop other punctuation.
pub fn normalize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn wer(reference: &str, hypothesis: &str) -> Wer {
    let r = normalize(reference);
    let h = normalize(hypothesis);
    // dp[i][j] = (total, subs, dels, ins) for r[..i] vs h[..j]
    let mut prev: Vec<(usize, usize, usize, usize)> = (0..=h.len()).map(|j| (j, 0, 0, j)).collect();
    for i in 1..=r.len() {
        let mut cur = vec![(i, 0, i, 0)];
        for j in 1..=h.len() {
            let (pd, ps, pdl, pi) = prev[j - 1];
            let sub = if r[i - 1] == h[j - 1] {
                (pd, ps, pdl, pi)
            } else {
                (pd + 1, ps + 1, pdl, pi)
            };
            let (ud, us, udl, ui) = prev[j];
            let del = (ud + 1, us, udl + 1, ui);
            let (ld, ls, ldl, li) = cur[j - 1];
            let ins = (ld + 1, ls, ldl, li + 1);
            cur.push(
                [sub, del, ins]
                    .into_iter()
                    .min_by_key(|x| x.0)
                    .unwrap_or(sub),
            );
        }
        prev = cur;
    }
    let (_, s, d, i) = prev.last().copied().unwrap_or((0, 0, 0, 0));
    Wer {
        ref_words: r.len(),
        substitutions: s,
        deletions: d,
        insertions: i,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_zero_ignoring_case_and_punctuation() {
        let w = wer("The quick brown fox.", "the QUICK brown, fox");
        assert_eq!(w.errors(), 0);
        assert_eq!(w.rate(), 0.0);
    }

    #[test]
    fn counts_each_error_kind() {
        let w = wer("a b c d", "a x c");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (1, 1, 0));
        let w = wer("a b", "a b c d");
        assert_eq!((w.substitutions, w.deletions, w.insertions), (0, 0, 2));
        assert_eq!(w.rate(), 1.0);
    }

    #[test]
    fn empty_reference_edge_cases() {
        assert_eq!(wer("", "").rate(), 0.0);
        assert_eq!(wer("", "oops").rate(), 1.0);
    }

    #[test]
    fn aggregates() {
        let mut t = Wer::default();
        t.add(&wer("a b c d", "a b c x"));
        t.add(&wer("e f", "e f"));
        assert_eq!((t.ref_words, t.errors()), (6, 1));
    }
}
