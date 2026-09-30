//! Subtitle (.srt / .vtt) parsing, sentence segmentation and Spanish tokenisation
//! for sentence mining.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleLine {
    pub index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleSentence {
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchedWord {
    pub token: String,
    pub word_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentenceMatch {
    pub words: Vec<MatchedWord>,
    pub coverage: f32,
}

pub trait WordMatcher {
    fn lookup(&self, form: &str) -> Vec<i64>;
}

pub struct InMemoryMatcher {
    pub map: std::collections::HashMap<String, Vec<i64>>,
}

impl WordMatcher for InMemoryMatcher {
    fn lookup(&self, form: &str) -> Vec<i64> {
        self.map.get(form).cloned().unwrap_or_default()
    }
}

fn timestamp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\d{1,2}):(\d{2}):(\d{2})[.,](\d{1,3})\s*-->\s*(\d{1,2}):(\d{2}):(\d{2})[.,](\d{1,3})")
            .unwrap()
    })
}

fn word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[a-záéíóúüñ]+").unwrap())
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"</?[a-zA-Z][^>]*>|\{\\[^}]*\}").unwrap())
}

fn to_ms(h: &str, m: &str, s: &str, frac: &str) -> i64 {
    let h: i64 = h.parse().unwrap_or(0);
    let m: i64 = m.parse().unwrap_or(0);
    let s: i64 = s.parse().unwrap_or(0);
    let frac = frac.parse::<i64>().unwrap_or(0) * 10_i64.pow(3 - frac.len().min(3) as u32);
    h * 3_600_000 + m * 60_000 + s * 1000 + frac
}

/// Parse SRT or WebVTT subtitle text into timed lines.
pub fn parse_subtitles(text: &str) -> Vec<SubtitleLine> {
    let re = timestamp_re();
    let mut lines = Vec::new();
    let mut index: i64 = 0;
    let mut cur: Option<(i64, i64, Vec<String>)> = None;

    let mut flush = |cur: &mut Option<(i64, i64, Vec<String>)>| {
        if let Some((s, e, texts)) = cur.take() {
            if !texts.is_empty() {
                index += 1;
                lines.push(SubtitleLine {
                    index,
                    start_ms: s,
                    end_ms: e,
                    text: texts.join(" "),
                });
            }
        }
    };
    for raw in text.trim_start_matches('\u{feff}').lines() {
        let raw = raw.trim_end_matches('\r');
        if raw.trim().is_empty() {
            flush(&mut cur);
            continue;
        }
        if raw.trim().chars().all(|c| c.is_ascii_digit()) && cur.is_none() {
            continue; // SRT sequence number before its timestamp line
        }
        if let Some(c) = re.captures(raw) {
            flush(&mut cur);
            cur = Some((
                to_ms(&c[1], &c[2], &c[3], &c[4]),
                to_ms(&c[5], &c[6], &c[7], &c[8]),
                Vec::new(),
            ));
            continue;
        }
        if let Some((_, _, texts)) = cur.as_mut() {
            let cleaned = tag_re().replace_all(raw, "").trim().to_string();
            if !cleaned.is_empty() {
                texts.push(cleaned);
            }
        }
    }
    flush(&mut cur);
    lines
}

fn is_noise(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    if line.contains('♪') || line.contains('♫') {
        return true;
    }
    if line.starts_with('[') || line.starts_with('(') {
        return true;
    }
    let letters: usize = chars.iter().filter(|c| c.is_alphabetic()).count();
    let upper: usize = chars.iter().filter(|c| c.is_uppercase()).count();
    letters > 0 && upper * 2 > letters
}

/// Merge subtitle lines into natural sentences.
pub fn lines_to_sentences(lines: &[SubtitleLine]) -> Vec<SubtitleSentence> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut start = 0i64;
    let mut end = 0i64;

    for line in lines {
        let text = line.text.trim();
        if text.is_empty() || is_noise(text) {
            continue;
        }
        if buf.is_empty() {
            start = line.start_ms;
            buf.push_str(text);
        } else {
            buf.push(' ');
            buf.push_str(text);
        }
        end = line.end_ms;

        let dur = end - start;
        let done = (buf.ends_with('.') || buf.ends_with('!') || buf.ends_with('?') || buf.ends_with('…'))
            || buf.len() >= 220
            || dur >= 12_000;
        if done {
            let s = clean_sentence(&buf);
            if let Some(s) = s {
                out.push(SubtitleSentence {
                    text: s,
                    start_ms: start,
                    end_ms: end,
                });
            }
            buf.clear();
        }
    }
    if !buf.is_empty() {
        if let Some(s) = clean_sentence(&buf) {
            out.push(SubtitleSentence {
                text: s,
                start_ms: start,
                end_ms: end,
            });
        }
    }
    out
}

fn clean_sentence(text: &str) -> Option<String> {
    let mut s = text.trim();
    s = s.trim_start_matches(|c: char| "-–—".contains(c));
    s = s.trim();
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() < 3 || words.len() > 40 {
        return None;
    }
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

/// Lowercase Spanish word tokens.
pub fn tokenize_es(sentence: &str) -> Vec<String> {
    word_re()
        .find_iter(&sentence.to_lowercase())
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Strip acute accents and diaeresis (keeps ñ).
pub fn strip_accents(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            'ü' => 'u',
            other => other,
        })
        .collect()
}

const STOPWORDS: &[&str] = &[
    "de", "la", "que", "el", "en", "y", "a", "los", "se", "del", "las", "un", "por", "con", "no",
    "una", "su", "para", "es", "al", "lo", "como", "más", "pero", "sus", "le", "ya", "o", "este",
    "sí", "porque", "esta", "entre", "cuando", "muy", "sin", "sobre", "también", "me", "hasta",
    "hay", "donde", "quien", "desde", "todo", "nos", "durante", "todos", "uno", "les", "ni",
    "contra", "otros", "ese", "eso", "ante", "e", "mí", "antes", "algunos", "qué", "unos", "yo",
    "otro", "otras", "otra", "él", "esa", "estos", "te", "ti", "les", "os", "uh", "eh", "ah",
    "oh", "va", "voy",
];

fn is_stopword(w: &str) -> bool {
    STOPWORDS.contains(&w) || STOPWORDS.contains(&strip_accents(w).as_str())
}

/// Match a sentence's tokens against a word matcher (lemma/inflection lookup).
pub fn match_sentence(sentence: &str, matcher: &dyn WordMatcher) -> SentenceMatch {
    let tokens = tokenize_es(sentence);
    let mut words = Vec::new();
    let mut matched_unique = 0usize;
    let mut total_unique = 0usize;
    let mut seen = std::collections::HashSet::new();

    for token in &tokens {
        if !seen.insert(token.clone()) {
            continue;
        }
        if is_stopword(token) {
            continue;
        }
        total_unique += 1;
        let mut ids = matcher.lookup(token);
        if ids.is_empty() {
            ids = matcher.lookup(&strip_accents(token));
        }
        if !ids.is_empty() {
            matched_unique += 1;
        }
        words.push(MatchedWord {
            token: token.clone(),
            word_ids: ids,
        });
    }
    let coverage = if total_unique == 0 {
        1.0
    } else {
        matched_unique as f32 / total_unique as f32
    };
    SentenceMatch { words, coverage }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_srt_with_tags() {
        let srt = "1\n00:00:01,000 --> 00:00:02,500\n<i>Hola,</i> ¿qué tal?\n\n2\n00:00:03,000 --> 00:00:04,000\nBien.\n";
        let lines = parse_subtitles(srt);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].start_ms, 1000);
        assert_eq!(lines[0].end_ms, 2500);
        assert_eq!(lines[0].text, "Hola, ¿qué tal?");
    }

    #[test]
    fn parses_vtt() {
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nHola mundo\n";
        let lines = parse_subtitles(vtt);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Hola mundo");
    }

    #[test]
    fn merges_lines_into_sentences() {
        let lines = vec![
            SubtitleLine { index: 1, start_ms: 0, end_ms: 1000, text: "No sé qué".into() },
            SubtitleLine { index: 2, start_ms: 1000, end_ms: 2000, text: "hacer con esto.".into() },
            SubtitleLine { index: 3, start_ms: 2000, end_ms: 3000, text: "♪ música ♪".into() },
        ];
        let sents = lines_to_sentences(&lines);
        assert_eq!(sents.len(), 1);
        assert_eq!(sents[0].text, "No sé qué hacer con esto.");
    }

    #[test]
    fn tokenizes_accents() {
        assert_eq!(tokenize_es("¡Mañana ñandú!"), vec!["mañana", "ñandú"]);
    }

    #[test]
    fn strips_accents_keeps_enye() {
        assert_eq!(strip_accents("áéíóúüñ"), "aeiouuñ");
    }

    #[test]
    fn matches_with_coverage() {
        let matcher = InMemoryMatcher {
            map: [("casa".to_string(), vec![1]), ("gato".to_string(), vec![2])]
                .into_iter()
                .collect(),
        };
        let m = match_sentence("El gato duerme en la casa grande", &matcher);
        // unique non-stopword tokens: gato, duerme, casa, grande => 4, matched: gato, casa
        assert!((m.coverage - 0.5).abs() < 1e-6);
        assert_eq!(m.words.len(), 4);
    }
}
