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
    /// Whether `form` is a dictionary spelling as written (no accent folding).
    fn knows(&self, form: &str) -> bool;
}

pub struct InMemoryMatcher {
    pub map: std::collections::HashMap<String, Vec<i64>>,
}

impl WordMatcher for InMemoryMatcher {
    fn lookup(&self, form: &str) -> Vec<i64> {
        self.map.get(form).cloned().unwrap_or_default()
    }

    fn knows(&self, form: &str) -> bool {
        self.map.contains_key(form)
    }
}

fn timestamp_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // hours are optional in WebVTT ("01:02.500 --> 01:04.000")
    RE.get_or_init(|| {
        Regex::new(r"(?:(\d{1,2}):)?(\d{2}):(\d{2})[.,](\d{1,3})\s*-->\s*(?:(\d{1,2}):)?(\d{2}):(\d{2})[.,](\d{1,3})")
            .unwrap()
    })
}

fn word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[a-záéíóúüñ]+").unwrap())
}

/// Words and numbers in any script, for telling sentences apart.
fn key_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[\p{L}\p{N}]+").unwrap())
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // <i>, </font>, WebVTT <c.yellow> and karaoke <00:00:01.000>, ASS {\an8}
    RE.get_or_init(|| Regex::new(r"</?[a-zA-Z][^>]*>|<\d[^>]*>|\{\\[^}]*\}").unwrap())
}

/// Sound and action descriptions: "(susurra)", "[música]".
fn description_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\([^)]*\)|\[[^\]]*\]").unwrap())
}

fn sentence_end_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[.!?…]+[»”\x22']?\s+").unwrap())
}

/// "...no estaríamos" opening a cue, or "verdad... ...no" where one cue
/// carries both halves: the second ellipsis only marks the continuation.
fn continuation_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // An ellipsis that opens the sentence or follows another one goes, with
    // the spaces after it. Anywhere else only one glued to the word it opens
    // does ("ir ...a la escuela"), so a pause written "hay ... no sé" stays.
    // An opening quote before it is kept. The regex crate has no lookahead,
    // so what follows is captured and put back; a straight quote there only
    // counts when a word starts in it, or `"No sé ..."` would lose its dots.
    RE.get_or_init(|| {
        Regex::new(&format!(
            r#"(^|{HANGING})([«“"]?)(?:{DOTS}\s*)+{OPENS}|(\s[«“"]?){DOTS}+{OPENS}"#
        ))
        .unwrap()
    })
}

/// The same marker where a line opens with it. There it is one however it
/// is spaced: "... no estaríamos aquí" after a line with no dots can't be
/// told from a pause once the two are joined.
fn leading_marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r#"^([«“"]?)(?:{DOTS}\s*)+{OPENS}"#)).unwrap())
}

/// Text that ends in a marker standing apart from the ellipsis before it:
/// "Si me hubieras dicho... ... ", with what carries on still to come.
fn marker_end_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r#"{HANGING}[«“"]?{DOTS}\s+$"#)).unwrap())
}

const DOTS: &str = r"(?:\.{2,}|…)";
/// The ellipsis a sentence was left hanging on, and the space after it.
const HANGING: &str = r#"(?:\.\.|…)[»”"']?\s+"#;
/// What a continuation marker stands before: the start of a word.
const OPENS: &str = r#"([\p{L}\p{N}¿¡«“]|"[\p{L}\p{N}¿¡])"#;

fn starts_with_dash(line: &str) -> bool {
    line.starts_with(['-', '–', '—'])
}

/// Subtitle files from macOS tools can be decomposed (NFD: "a" + U+0301),
/// which the letter classes here don't match.
fn nfc(s: &str) -> std::borrow::Cow<'_, str> {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(s)
}

fn to_ms(h: Option<&str>, m: &str, s: &str, frac: &str) -> i64 {
    let h = h.unwrap_or("0");
    let h: i64 = h.parse().unwrap_or(0);
    let m: i64 = m.parse().unwrap_or(0);
    let s: i64 = s.parse().unwrap_or(0);
    let frac = frac.parse::<i64>().unwrap_or(0) * 10_i64.pow(3 - frac.len().min(3) as u32);
    h * 3_600_000 + m * 60_000 + s * 1000 + frac
}

/// Parse SRT or WebVTT subtitle text into timed lines, one per speaker turn.
/// Text with no cue timings at all is read as plain prose instead.
pub fn parse_subtitles(text: &str) -> Vec<SubtitleLine> {
    let text = nfc(text);
    let re = timestamp_re();
    let raw_lines: Vec<&str> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    if !raw_lines.iter().any(|l| re.is_match(l)) {
        return parse_prose(&raw_lines);
    }

    let mut lines = Vec::new();
    let mut index: i64 = 0;
    let mut cur: Option<(i64, i64, Vec<String>)> = None;

    // A cue becomes one line per speaker turn: in "- ¿Vienes?\n- Sí." each
    // dash starts a different person's line, which mustn't become one sentence.
    let mut flush = |cur: &mut Option<(i64, i64, Vec<String>)>| {
        if let Some((s, e, texts)) = cur.take() {
            let mut turns: Vec<String> = Vec::new();
            for t in texts {
                match turns.last_mut() {
                    Some(last) if !starts_with_dash(&t) => {
                        last.push(' ');
                        last.push_str(&t);
                    }
                    _ => turns.push(t),
                }
            }
            for text in turns {
                index += 1;
                lines.push(SubtitleLine { index, start_ms: s, end_ms: e, text });
            }
        }
    };
    for (i, raw) in raw_lines.iter().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            flush(&mut cur);
            continue;
        }
        if let Some(c) = re.captures(raw) {
            flush(&mut cur);
            cur = Some((
                to_ms(c.get(1).map(|m| m.as_str()), &c[2], &c[3], &c[4]),
                to_ms(c.get(5).map(|m| m.as_str()), &c[6], &c[7], &c[8]),
                Vec::new(),
            ));
            continue;
        }
        // SRT's sequence number, which sloppy files put straight after the
        // previous cue's text with no blank line between
        let next_is_timing = raw_lines.get(i + 1).is_some_and(|next| re.is_match(next));
        if trimmed.chars().all(|c| c.is_ascii_digit()) && (cur.is_none() || next_is_timing) {
            continue;
        }
        if let Some((_, _, texts)) = cur.as_mut() {
            let untagged = tag_re().replace_all(raw, "");
            let spoken = description_re().replace_all(&untagged, "");
            let cleaned = spoken.split_whitespace().collect::<Vec<_>>().join(" ");
            // a dash left alone by "- (risas)" is no speaker turn
            if !cleaned.trim_start_matches(['-', '–', '—']).trim().is_empty() {
                texts.push(cleaned);
            }
        }
    }
    flush(&mut cur);
    lines
}

/// Plain text: one untimed line per line of text; `lines_to_sentences`
/// splits and joins them into sentences like any subtitle.
fn parse_prose(raw_lines: &[&str]) -> Vec<SubtitleLine> {
    raw_lines
        .iter()
        .map(|raw| raw.trim())
        .filter(|raw| !raw.is_empty())
        .enumerate()
        .map(|(i, text)| SubtitleLine {
            index: i as i64 + 1,
            start_ms: 0,
            end_ms: 0,
            text: text.to_string(),
        })
        .collect()
}

fn is_noise(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    if line.contains('♪') || line.contains('♫') {
        return true;
    }
    let letters: usize = chars.iter().filter(|c| c.is_alphabetic()).count();
    let upper: usize = chars.iter().filter(|c| c.is_uppercase()).count();
    letters > 0 && upper * 2 > letters
}

/// Merge subtitle lines into natural sentences, and split lines that hold
/// more than one.
pub fn lines_to_sentences(lines: &[SubtitleLine]) -> Vec<SubtitleSentence> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut start = 0i64;
    let mut end = 0i64;

    for (li, line) in lines.iter().enumerate() {
        let text = line.text.trim();
        if text.is_empty() || is_noise(text) {
            continue;
        }
        // a new speaker's turn starts a new sentence
        if starts_with_dash(text) && !buf.is_empty() {
            emit(&mut out, &mut buf, start, end);
        }
        // dots that open a line only say that it carries on
        let text = leading_marker_re().replace(text, "${1}${2}");
        let pieces = split_sentences(&text);
        let last = pieces.len().saturating_sub(1);
        for (i, piece) in pieces.into_iter().enumerate() {
            // Dots alone, after a line that trailed off, add nothing to it:
            // a cue that is only a pause is passed by.
            let marker = ends_in_ellipsis(&buf)
                && opens_with_continuation(piece)
                && !piece.contains(char::is_alphanumeric);
            if !marker {
                if buf.is_empty() {
                    start = line.start_ms;
                } else {
                    buf.push(' ');
                }
                buf.push_str(piece);
                end = line.end_ms;
            }

            // A short sentence inside a line ("Hola. ¿Qué tal?", "Vale. Te
            // lo prometo.") stays with the next one: alone it's too short to
            // keep, and together they read as one exchange.
            let ended = buf.trim_end_matches(CLOSING_QUOTES).ends_with(['.', '!', '?', '…']);
            // "Si me hubieras dicho la verdad..." / "...no estaríamos aquí":
            // a line that trails off and is picked up by the next one is one
            // sentence, within the time a sentence may span.
            let carries_on = i == last
                && ends_in_ellipsis(&buf)
                && next_spoken(&lines[li + 1..]).is_some_and(|next| {
                    opens_with_continuation(next.text.trim()) && next.end_ms - start < 12_000
                });
            let done = (ended && !carries_on && (i == last || word_count(&buf) >= MIN_WORDS))
                || buf.len() >= 220
                || end - start >= 12_000;
            if done {
                emit(&mut out, &mut buf, start, end);
            }
        }
    }
    if !buf.is_empty() {
        emit(&mut out, &mut buf, start, end);
    }
    out
}

const MIN_WORDS: usize = 3;
const CLOSING_QUOTES: [char; 4] = ['»', '”', '"', '\''];

/// Titles and abbreviations whose full stop doesn't end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "sr", "sra", "srta", "sres", "dr", "dra", "dña", "ud", "uds", "vd", "vds", "prof", "lic",
];

fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

fn ends_in_ellipsis(buf: &str) -> bool {
    let buf = buf.trim_end_matches(CLOSING_QUOTES);
    buf.ends_with("..") || buf.ends_with('…')
}

/// Whether a line carries on the sentence the line before left hanging: it
/// opens with an ellipsis of its own, or in lowercase. A new speaker's line
/// never does.
fn opens_with_continuation(line: &str) -> bool {
    if starts_with_dash(line) {
        return false;
    }
    let line = line.trim_start_matches(['«', '“', '"']);
    line.starts_with("..") || line.starts_with('…') || line.starts_with(char::is_lowercase)
}

/// The next line `lines_to_sentences` will read, past blanks and songs.
fn next_spoken(rest: &[SubtitleLine]) -> Option<&SubtitleLine> {
    rest.iter().find(|line| {
        let text = line.text.trim();
        !text.is_empty() && !is_noise(text)
    })
}

fn emit(out: &mut Vec<SubtitleSentence>, buf: &mut String, start_ms: i64, end_ms: i64) {
    if let Some(text) = clean_sentence(buf) {
        out.push(SubtitleSentence { text, start_ms, end_ms });
    }
    buf.clear();
}

/// Splits `text` where one sentence ends and another starts inside it: after
/// . ! ? or … when the next word opens a sentence (a capital, ¿ or ¡).
/// "Pues... no sé", "el Sr. García" and "J. R. R. Tolkien" stay whole.
fn split_sentences(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut from = 0;
    for m in sentence_end_re().find_iter(text) {
        // "dicho... ... Juan no estaría aquí": dots straight after an
        // ellipsis end nothing, they mark that the sentence carries on
        if marker_end_re().is_match(&text[..m.end()]) {
            continue;
        }
        let next = text[m.end()..]
            .trim_start_matches(['«', '“', '"', '-', '–', '—', ' '])
            .chars()
            .next();
        if !next.is_some_and(|c| c.is_uppercase() || c == '¿' || c == '¡') {
            continue;
        }
        if m.as_str().trim_end().trim_end_matches(CLOSING_QUOTES) == "." {
            let before = text[..m.start()]
                .rsplit(char::is_whitespace)
                .next()
                .unwrap_or("")
                .trim_start_matches(|c: char| !c.is_alphanumeric());
            let initial = before.chars().count() == 1 && before.starts_with(char::is_uppercase);
            if initial || ABBREVIATIONS.contains(&before.to_lowercase().as_str()) {
                continue;
            }
        }
        pieces.push(text[from..m.end()].trim());
        from = m.end();
    }
    pieces.push(text[from..].trim());
    pieces.retain(|p| !p.is_empty());
    pieces
}

fn clean_sentence(text: &str) -> Option<String> {
    let mut s = text.trim();
    s = s.trim_start_matches(|c: char| "-–—".contains(c));
    s = s.trim();
    let unmarked = continuation_re().replace_all(s, "${1}${2}${3}${4}${5}");
    let s = unmarked.trim();
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
        .find_iter(&nfc(sentence).to_lowercase())
        .map(|m| m.as_str().to_string())
        .collect()
}

/// What two copies of a sentence share however they're written: its words
/// and numbers, lowercased and without accents, so "¡Vale, vamos!" and
/// "vale... vamos" from two subtitle files count as one sentence, and
/// "Tengo 30 años" and "Tengo 40 años" as two.
pub fn sentence_key(sentence: &str) -> String {
    key_word_re()
        .find_iter(&nfc(sentence).to_lowercase())
        .map(|m| strip_accents(m.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
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
    "otro", "otras", "otra", "él", "esa", "estos", "te", "ti", "os", "uh", "eh", "ah",
    "oh", "va", "voy", "cómo", "dónde", "cuándo", "quién", "éste", "ésta", "ése", "ésa", "éstos",
    "ó",
];

/// A spelling the dictionary knows is judged as written: "está", "sé" and
/// "té" are words, not "esta", "se" and "te". An unknown spelling ("tí",
/// "éso") is a mis-accented stopword.
fn is_stopword(w: &str, matcher: &dyn WordMatcher) -> bool {
    STOPWORDS.contains(&w) || (!matcher.knows(w) && STOPWORDS.contains(&strip_accents(w).as_str()))
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
        if is_stopword(token, matcher) {
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
        // nothing in it to learn
        0.0
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

    fn sentences(subs: &str) -> Vec<String> {
        lines_to_sentences(&parse_subtitles(subs)).into_iter().map(|s| s.text).collect()
    }

    /// An .srt with one two-second cue per text, back to back from 0:01.
    fn cues(texts: &[&str]) -> String {
        texts
            .iter()
            .enumerate()
            .map(|(i, text)| format!("{}\n00:00:{:02},000 --> 00:00:{:02},000\n{text}\n\n", i + 1, 1 + 2 * i, 3 + 2 * i))
            .collect()
    }

    #[test]
    fn parses_vtt_timings_without_hours() {
        let vtt = "WEBVTT\n\n01:02.500 --> 01:04.000 align:start\nHola mundo\n\nintro\n00:01:05.000 --> 00:01:06.000\n<c.yellow>Adiós</c>\n";
        let lines = parse_subtitles(vtt);
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].start_ms, lines[0].end_ms), (62_500, 64_000));
        assert_eq!(lines[1].text, "Adiós");
    }

    #[test]
    fn cue_numbers_do_not_leak_when_blank_lines_are_missing() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nNo quiero ir\n2\n00:00:03,000 --> 00:00:04,000\na la escuela hoy.\n";
        assert_eq!(sentences(srt), ["No quiero ir a la escuela hoy."]);
    }

    #[test]
    fn sound_descriptions_go_but_the_words_stay() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\n(susurra) No se lo digas a nadie.\n\n2\n00:00:04,000 --> 00:00:05,000\n[música]\n\n3\n00:00:06,000 --> 00:00:08,000\n<00:00:06.500>Vale, te lo prometo.\n";
        assert_eq!(sentences(srt), ["No se lo digas a nadie.", "Vale, te lo prometo."]);
    }

    #[test]
    fn two_speakers_in_one_cue_are_two_sentences() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\n- ¿Vienes a la fiesta?\n- Sí, ahora mismo voy.\n";
        assert_eq!(sentences(srt), ["¿Vienes a la fiesta?", "Sí, ahora mismo voy."]);
    }

    #[test]
    fn two_sentences_in_one_cue_are_two_sentences() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nNo lo sé. Pregúntale a tu madre, que ella sabe.\n\n2\n00:00:05,000 --> 00:00:06,000\nNo quiero ir\n\n3\n00:00:06,000 --> 00:00:08,000\na la escuela. ¿Vienes conmigo o no?\n";
        let found = lines_to_sentences(&parse_subtitles(srt));
        let texts: Vec<&str> = found.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            texts,
            ["No lo sé.", "Pregúntale a tu madre, que ella sabe.", "No quiero ir a la escuela.", "¿Vienes conmigo o no?"]
        );
        assert_eq!((found[2].start_ms, found[2].end_ms), (5_000, 8_000));
        assert_eq!(found[3].start_ms, 6_000);
    }

    #[test]
    fn short_sentences_and_abbreviations_are_not_split_off() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nHola. ¿Qué tal?\n\n2\n00:00:04,000 --> 00:00:06,000\nPues... no sé qué decirte.\n\n3\n00:00:07,000 --> 00:00:09,000\nBuenos días, Sr. García. ¿Cómo está usted?\n\n4\n00:00:10,000 --> 00:00:12,000\nLo escribió J. R. R. Tolkien, creo.\n";
        assert_eq!(
            sentences(srt),
            [
                "Hola. ¿Qué tal?",
                "Pues... no sé qué decirte.",
                "Buenos días, Sr. García.",
                "¿Cómo está usted?",
                "Lo escribió J. R. R. Tolkien, creo.",
            ]
        );
    }

    #[test]
    fn an_ellipsis_continued_in_the_next_cue_is_one_sentence() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nSi me hubieras dicho la verdad...\n\n2\n00:00:03,500 --> 00:00:05,000\n...no estaríamos aquí ahora.\n";
        let found = lines_to_sentences(&parse_subtitles(srt));
        let texts: Vec<&str> = found.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Si me hubieras dicho la verdad... no estaríamos aquí ahora."]);
        assert_eq!((found[0].start_ms, found[0].end_ms), (1_000, 5_000));
    }

    #[test]
    fn a_short_first_half_stays_with_its_continuation() {
        assert_eq!(sentences(&cues(&["Y entonces...", "...se fue sin decir nada."])), ["Y entonces... se fue sin decir nada."]);
        assert_eq!(sentences(&cues(&["Y entonces…", "…se fue sin decir nada."])), ["Y entonces… se fue sin decir nada."]);
        assert_eq!(sentences(&cues(&["Y entonces...", "se fue sin decir nada."])), ["Y entonces... se fue sin decir nada."]);
    }

    #[test]
    fn an_ellipsis_before_a_new_sentence_or_speaker_still_ends_it() {
        assert_eq!(
            sentences(&cues(&["No sé si me entiendes...", "Mañana te llamo, ¿vale?"])),
            ["No sé si me entiendes...", "Mañana te llamo, ¿vale?"]
        );
        assert_eq!(
            sentences(&cues(&["Si me hubieras dicho la verdad...", "- ...no, déjalo ya."])),
            ["Si me hubieras dicho la verdad...", "no, déjalo ya."]
        );
        assert_eq!(sentences(&cues(&["Pero...", "¿Qué haces aquí tan tarde?"])), ["¿Qué haces aquí tan tarde?"]);
        // nor do two ellipses in a line make the second one a marker
        assert_eq!(sentences(&cues(&["Bueno... no sé... Mañana te lo digo."])), ["Bueno... no sé...", "Mañana te lo digo."]);
        assert!(!opens_with_continuation("- ...no, déjalo ya."));
        assert!(!opens_with_continuation("¡no me digas!"));
    }

    #[test]
    fn continuation_marks_are_not_kept() {
        assert_eq!(sentences(&cues(&["...no estaríamos aquí ahora."])), ["no estaríamos aquí ahora."]);
        // both halves in one cue, on two lines
        assert_eq!(
            sentences(&cues(&["Si me hubieras dicho la verdad...\n...no estaríamos aquí."])),
            ["Si me hubieras dicho la verdad... no estaríamos aquí."]
        );
        // and when the marker stands apart before a name, on two lines or
        // on one, or after a pause that got a line to itself
        for both in [
            "Si me hubieras dicho...\n... Juan no estaría aquí.",
            "Si me hubieras dicho... ... Juan no estaría aquí.",
            "Si me hubieras dicho...\n...\n... Juan no estaría aquí.",
        ] {
            assert_eq!(sentences(&cues(&[both])), ["Si me hubieras dicho... Juan no estaría aquí."]);
        }
        // the marker goes, the quote it sits in stays
        assert_eq!(
            sentences(&cues(&["Le dije: \"Espera...", "...que ya voy\", y se fue."])),
            ["Le dije: \"Espera... que ya voy\", y se fue."]
        );
        assert_eq!(
            sentences(&cues(&["Yo creo que...", "\"...no vendrá nunca más.\""])),
            ["Yo creo que... \"no vendrá nunca más.\""]
        );
        // also when the marker stands apart before a name, in the next cue or in the same one
        for quoted in [&["Yo creo que...", "\"... Juan no vendrá nunca más.\""][..], &["Yo creo que...\n\"... Juan no vendrá nunca más.\""]] {
            assert_eq!(sentences(&cues(quoted)), ["Yo creo que... \"Juan no vendrá nunca más.\""]);
        }
        assert_eq!(
            sentences(&cues(&["Y entonces me dijo...", "...\"no vuelvas más\"."])),
            ["Y entonces me dijo... \"no vuelvas más\"."]
        );
        // dots that open a cue are a marker however they're spaced, whether
        // or not the cue before ends in dots
        for marked in ["...a la escuela hoy.", "… a la escuela hoy.", "...\n... a la escuela hoy."] {
            assert_eq!(sentences(&cues(&["No quiero ir", marked])), ["No quiero ir a la escuela hoy."]);
        }
        assert_eq!(
            sentences(&cues(&["Si me hubieras dicho la verdad", "... no estaríamos aquí."])),
            ["Si me hubieras dicho la verdad no estaríamos aquí."]
        );
        // inside a cue, dots glued to the word they open are one too
        assert_eq!(sentences(&cues(&["No quiero ir\n...a la escuela hoy."])), ["No quiero ir a la escuela hoy."]);
        // a pause inside a sentence is not a marker, nor are dots that end one
        for kept in [
            "Pues... no sé qué decirte.",
            "Yo ... no sé qué decirte.",
            "Me dijo: \"No lo sé ...\"",
            "Me miró y dijo \"...\" sin más.",
        ] {
            assert_eq!(sentences(&cues(&[kept])), [kept]);
        }
        // nor is the first half's own ellipsis, however it's spaced
        assert_eq!(sentences(&cues(&["En el armario hay ...", "no voy a decir qué."])), ["En el armario hay ... no voy a decir qué."]);
    }

    #[test]
    fn a_marked_continuation_carries_over_a_song_and_a_capital() {
        let joined = ["Si me hubieras dicho... Juan no estaría aquí."];
        assert_eq!(sentences(&cues(&["Si me hubieras dicho...", "♪ la la la ♪", "...Juan no estaría aquí."])), joined);
        // the same with a space after the marker, which must not end a sentence
        assert_eq!(sentences(&cues(&["Si me hubieras dicho...", "... Juan no estaría aquí."])), joined);
        // and after a line with no dots, which is carried on like any line
        // without a full stop; the marker leaves nothing behind
        assert_eq!(
            sentences(&cues(&["Y entonces me dijo que", "... Juan no estaría aquí."])),
            ["Y entonces me dijo que Juan no estaría aquí."]
        );
    }

    #[test]
    fn a_cue_that_is_only_a_pause_adds_nothing() {
        assert_eq!(sentences(&cues(&["...", "...no sé qué decirte."])), ["no sé qué decirte."]);
        assert_eq!(
            sentences(&cues(&["No sé qué decirte...", "...", "...pero mañana te llamo."])),
            ["No sé qué decirte... pero mañana te llamo."]
        );
        // and it joins nothing by itself
        let found = lines_to_sentences(&parse_subtitles(&cues(&["No sé qué decirte...", "...", "Mañana te llamo."])));
        let found: Vec<(&str, i64)> = found.iter().map(|s| (s.text.as_str(), s.end_ms)).collect();
        assert_eq!(found, [("No sé qué decirte...", 3_000), ("Mañana te llamo.", 7_000)]);
        // after a line with no end of its own, it is that line trailing off
        assert_eq!(
            sentences(&cues(&["No sé qué decirte", "...", "Mañana te llamo."])),
            ["No sé qué decirte ...", "Mañana te llamo."]
        );
    }

    #[test]
    fn a_continuation_does_not_span_a_long_gap() {
        let srt = "1\n00:00:00,000 --> 00:00:02,000\nMe dijo que vendría...\n\n2\n00:00:20,000 --> 00:00:22,000\npero al final no vino.\n";
        assert_eq!(sentences(srt), ["Me dijo que vendría...", "pero al final no vino."]);
    }

    #[test]
    fn plain_text_is_split_into_sentences() {
        let txt = "Mi hermano vive en Madrid. ¿Tú dónde vives ahora?\n\nYo vivo en Sevilla con mi familia y mis dos perros.";
        assert_eq!(
            sentences(txt),
            ["Mi hermano vive en Madrid.", "¿Tú dónde vives ahora?", "Yo vivo en Sevilla con mi familia y mis dos perros."]
        );
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
    fn sentence_keys_ignore_punctuation_case_and_accents() {
        assert_eq!(sentence_key("¡Vale, vámonos ya!"), sentence_key("vale... vamonos ya"));
        assert_ne!(sentence_key("Vale, vámonos ya."), sentence_key("Vale, vámonos mañana."));
        assert_ne!(sentence_key("El año que viene."), sentence_key("El ano que viene."));
    }

    #[test]
    fn sentence_keys_keep_numbers() {
        assert_ne!(sentence_key("Tengo 30 años y dos hijos."), sentence_key("Tengo 40 años y dos hijos."));
        assert_eq!(sentence_key("¡Ya tengo 30 años, tío!"), sentence_key("ya tengo 30 años tio"));
        assert_ne!(sentence_key("1, 2, 3."), sentence_key("3, 2, 1."));
    }

    #[test]
    fn decomposed_text_reads_like_composed() {
        // written with escapes so no editor can recompose the first one
        let nfd = "Man\u{303}ana esta\u{301} en casa, ¿no?";
        let nfc = "Ma\u{f1}ana est\u{e1} en casa, ¿no?";
        assert_eq!(tokenize_es(nfd), tokenize_es(nfc));
        assert_eq!(tokenize_es(nfd), ["mañana", "está", "en", "casa", "no"]);
        assert_eq!(sentence_key(nfd), sentence_key(nfc));
        let lines = parse_subtitles(&cues(&[nfd]));
        assert_eq!(lines[0].text, nfc);
        // plain text takes the same path
        assert_eq!(parse_subtitles(nfd)[0].text, nfc);
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

    fn matcher_of(forms: &[&str]) -> InMemoryMatcher {
        InMemoryMatcher {
            map: forms.iter().enumerate().map(|(i, form)| (form.to_string(), vec![i as i64 + 1])).collect(),
        }
    }

    fn tokens(m: &SentenceMatch) -> Vec<&str> {
        m.words.iter().map(|w| w.token.as_str()).collect()
    }

    #[test]
    fn accented_verbs_and_nouns_are_not_stopwords() {
        let matcher = matcher_of(&["madre", "casa", "está", "sé", "té", "dé", "quiere", "si", "mi"]);
        let m = match_sentence("Mi madre está en casa y no sé si quiere té. Dé igual.", &matcher);
        let found = tokens(&m);
        for word in ["está", "sé", "té", "dé"] {
            assert!(found.contains(&word), "{word} skipped as a stopword: {found:?}");
        }
        assert!(m.words.iter().filter(|w| w.token != "igual").all(|w| !w.word_ids.is_empty()));
    }

    #[test]
    fn accented_function_words_and_misspellings_stay_stopwords() {
        // "tí" is a common misspelling of "ti" that no dictionary lists
        let matcher = matcher_of(&["cómo", "dónde", "éste", "casa"]);
        let m = match_sentence("¿Cómo y dónde está éste? Para tí, la casa.", &matcher);
        assert_eq!(tokens(&m), ["casa"]);
        // a file written without accents: "esta" may be "está", and can't be told apart
        assert_eq!(tokens(&match_sentence("Mi madre esta en casa.", &matcher_of(&["está", "casa"]))), ["mi", "madre", "casa"]);
    }

    #[test]
    fn a_sentence_with_nothing_to_learn_has_no_coverage() {
        let matcher = matcher_of(&["sí", "ya", "voy", "casa"]);
        assert_eq!(match_sentence("Sí, sí, ya voy.", &matcher).coverage, 0.0);
        assert_eq!(match_sentence("1, 2, 3.", &matcher).coverage, 0.0);
    }

    #[test]
    fn stopwords_are_listed_once() {
        let mut unique = STOPWORDS.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), STOPWORDS.len());
    }
}
