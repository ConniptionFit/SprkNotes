//! Helpers real engines share to turn raw token pieces into clean streamed text: whole UTF-8
//! characters only, and stop strings that never leak into the output even when they arrive in
//! several pieces. Pure, so the llama.cpp adapter stays thin and this part tests on Linux.

/// Collects bytes and hands back only complete UTF-8 characters.
#[derive(Debug, Default)]
pub struct Utf8Pending(Vec<u8>);

impl Utf8Pending {
    /// Adds bytes and returns the text that is now complete. Invalid bytes become U+FFFD.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.0.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.0) {
                Ok(s) => {
                    out.push_str(s);
                    self.0.clear();
                    return out;
                }
                Err(e) => {
                    let good = e.valid_up_to();
                    // The prefix is valid by construction, so the lossy conversion changes nothing.
                    out.push_str(&String::from_utf8_lossy(&self.0[..good]));
                    match e.error_len() {
                        // Bad bytes in the middle: replace them and keep going.
                        Some(n) => {
                            out.push('\u{FFFD}');
                            self.0.drain(..good + n);
                        }
                        // An unfinished character at the end: wait for more bytes.
                        None => {
                            self.0.drain(..good);
                            return out;
                        }
                    }
                }
            }
        }
    }
}

/// Cuts the stream at the first stop string and holds back text that might be the start of one.
#[derive(Debug)]
pub struct StopFilter {
    stops: Vec<String>,
    held: String,
    done: bool,
}

impl StopFilter {
    pub fn new(stops: &[String]) -> Self {
        Self {
            stops: stops.iter().filter(|s| !s.is_empty()).cloned().collect(),
            held: String::new(),
            done: false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Feeds text. Returns what is safe to show now. After a stop string was seen, returns
    /// nothing more.
    pub fn push(&mut self, text: &str) -> String {
        if self.done {
            return String::new();
        }
        self.held.push_str(text);
        if let Some(cut) = self
            .stops
            .iter()
            .filter_map(|s| self.held.find(s.as_str()))
            .min()
        {
            self.done = true;
            let out = self.held[..cut].to_string();
            self.held.clear();
            return out;
        }
        // Keep the longest tail that is still a possible beginning of some stop string.
        let mut keep = 0;
        for s in &self.stops {
            for len in (1..s.len()).rev() {
                if !s.is_char_boundary(len) {
                    continue;
                }
                if self.held.ends_with(&s[..len]) {
                    keep = keep.max(len);
                    break;
                }
            }
        }
        let split = self.held.len() - keep;
        let out = self.held[..split].to_string();
        self.held.drain(..split);
        out
    }

    /// The stream ended without a stop string: release what was held back.
    pub fn finish(&mut self) -> String {
        if self.done {
            return String::new();
        }
        self.done = true;
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stops(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn utf8_waits_for_the_rest_of_a_character() {
        let mut p = Utf8Pending::default();
        let euro = "€".as_bytes(); // 3 bytes
        assert_eq!(p.push(&euro[..1]), "");
        assert_eq!(p.push(&euro[1..2]), "");
        assert_eq!(p.push(&euro[2..]), "€");
        assert_eq!(p.push(b"ok"), "ok");
    }

    #[test]
    fn utf8_replaces_bad_bytes_and_keeps_going() {
        let mut p = Utf8Pending::default();
        assert_eq!(p.push(b"a\xFFb"), "a\u{FFFD}b");
    }

    #[test]
    fn stop_text_is_cut_even_when_split_across_pieces() {
        let mut f = StopFilter::new(&stops(&["<|im_end|>"]));
        let mut out = String::new();
        for piece in ["Weekly ", "sync<|im", "_end|>", " ignored"] {
            out.push_str(&f.push(piece));
        }
        out.push_str(&f.finish());
        assert_eq!(out, "Weekly sync");
        assert!(f.is_done());
    }

    #[test]
    fn a_near_miss_is_released_when_it_turns_out_not_to_be_a_stop() {
        let mut f = StopFilter::new(&stops(&["<|im_end|>"]));
        let mut out = String::new();
        for piece in ["a <|im", "ortant", " point"] {
            out.push_str(&f.push(piece));
        }
        out.push_str(&f.finish());
        assert_eq!(out, "a <|imortant point");
    }

    #[test]
    fn newline_stop_for_titles_and_multiple_stops() {
        let mut f = StopFilter::new(&stops(&["\n", "<|im_end|>"]));
        let a = f.push("Budget review");
        let b = f.push("\nsecond line");
        assert_eq!(format!("{a}{b}"), "Budget review");
    }

    #[test]
    fn finish_releases_held_text_and_no_stops_means_passthrough() {
        let mut f = StopFilter::new(&stops(&["END"]));
        assert_eq!(f.push("tail E"), "tail ");
        assert_eq!(f.finish(), "E");
        let mut g = StopFilter::new(&[]);
        assert_eq!(g.push("anything"), "anything");
    }
}
