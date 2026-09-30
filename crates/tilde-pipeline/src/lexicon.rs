//! Spanish lexicon from Wiktionary (kaikki.org JSONL): which written forms
//! belong to which lemma, how common each lemma is, and what to store for it.
//!
//! Two passes over the dump: `Index::build` keeps a compact summary of every
//! written word (enough to lemmatize and rank the frequency list), then
//! `details` re-reads it for just the selected lemmas to collect glosses,
//! inflections and conjugation tables.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::BufRead;

pub const PERSONS: [&str; 6] = [
    "yo",
    "tú",
    "él/ella/usted",
    "nosotros",
    "vosotros",
    "ellos/ellas/ustedes",
];

/// Very common irregular verbs. A written form that can belong to one of these
/// ("era", "haya", "fue") is credited to the verb, not to a rarer homograph.
const CORE_VERBS: [&str; 12] = [
    "ser", "estar", "haber", "ir", "tener", "hacer", "poder", "decir", "ver", "dar", "saber",
    "querer",
];

/// Subtitle noise that Wiktionary happens to list as Spanish interjections.
const FILLERS: [&str; 21] = [
    "oh", "eh", "ah", "uh", "ay", "hey", "ok", "okay", "wow", "bah", "uf", "mmm", "mm", "hmm",
    "ja", "jaja", "jajaja", "huh", "ey", "yeah", "yes",
];

/// Words kept as their own entry although Wiktionary defines them as a form.
fn keep_as_word(word: &str) -> bool {
    include_str!("../curation/lemmas.txt")
        .lines()
        .any(|l| !l.starts_with('#') && l.trim() == word)
}

/// Forms credited to what they inflect rather than to their own rare sense.
fn prefer_inflection(form: &str) -> bool {
    include_str!("../curation/inflections.txt")
        .lines()
        .any(|l| !l.starts_with('#') && l.trim() == form)
}

const ABBREVIATIONS: [(&str, &str); 9] = [
    ("sr.", "señor"),
    ("sra.", "señora"),
    ("srta.", "señorita"),
    ("dr.", "doctor"),
    ("dra.", "doctora"),
    ("ud.", "usted"),
    ("uds.", "ustedes"),
    ("vd.", "usted"),
    ("vds.", "ustedes"),
];

/// Wiktionary region labels outside Spain.
const REGION_TAGS: [&str; 33] = [
    "Latin-America",
    "Mexico",
    "Argentina",
    "Rioplatense",
    "Colombia",
    "Chile",
    "Peru",
    "Venezuela",
    "Cuba",
    "Caribbean",
    "Central-America",
    "Puerto-Rico",
    "Bolivia",
    "Paraguay",
    "Uruguay",
    "Ecuador",
    "Costa-Rica",
    "Dominican-Republic",
    "Guatemala",
    "Honduras",
    "Nicaragua",
    "Panama",
    "El-Salvador",
    "Andes",
    "US",
    "Philippines",
    "South-America",
    "Equatorial-Guinea",
    "New-Mexico",
    "Louisiana",
    "Canary-Islands",
    "Andalusia",
    "Latin-American",
];

const MARGINAL_TAGS: [&str; 7] = [
    "obsolete",
    "archaic",
    "dated",
    "rare",
    "historical",
    "nonstandard",
    "uncommon",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// Inflecting content words: nouns, verbs, adjectives, adverbs.
    Lex,
    /// Function words, kept as themselves (never folded into another lemma).
    Closed,
    Intj,
}

fn class_of(pos: &str) -> Option<Class> {
    match pos {
        "noun" | "verb" | "adj" | "adv" => Some(Class::Lex),
        "pron" | "det" | "article" | "prep" | "conj" | "contraction" | "num" | "particle" => {
            Some(Class::Closed)
        }
        "intj" => Some(Class::Intj),
        _ => None,
    }
}

// --- raw kaikki JSON ---------------------------------------------------------

#[derive(Deserialize)]
struct LiteEntry {
    #[serde(default)]
    word: String,
    #[serde(default)]
    pos: String,
    #[serde(default)]
    lang_code: String,
    #[serde(default)]
    senses: Vec<RawSense>,
}

#[derive(Deserialize)]
struct FullEntry {
    #[serde(default)]
    word: String,
    #[serde(default)]
    pos: String,
    #[serde(default)]
    lang_code: String,
    #[serde(default)]
    senses: Vec<RawSense>,
    #[serde(default)]
    forms: Vec<RawForm>,
}

#[derive(Deserialize)]
struct RawSense {
    #[serde(default)]
    glosses: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    form_of: Vec<RawLink>,
    #[serde(default)]
    alt_of: Vec<RawLink>,
}

#[derive(Deserialize)]
struct RawLink {
    #[serde(default)]
    word: String,
}

#[derive(Deserialize)]
struct RawForm {
    #[serde(default)]
    form: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Lemma,
    /// Inflection of another word ("plural of casa").
    Form,
    /// Spelling or abbreviation variant ("apocopic form of tuyo").
    Alt,
}

struct Sense<'a> {
    kind: Kind,
    gloss: &'a str,
    regional: bool,
    spain: bool,
    marginal: bool,
    vulgar: bool,
    letter: bool,
    /// "comparative degree of malo: worse": a word of its own for learners.
    comparative: bool,
    /// "apocopic form of bueno": folded into the full form.
    apocopic: bool,
}

fn sense(s: &RawSense) -> Option<Sense<'_>> {
    let has = |t: &str| s.tags.iter().any(|x| x == t);
    if has("misspelling") {
        return None;
    }
    // subsenses list the whole path ["Figurative senses", "to come"]; the last is the sense itself
    let gloss = s.glosses.last().map(String::as_str).unwrap_or("");
    let kind = if !s.form_of.is_empty() || has("form-of") {
        Kind::Form
    } else if !s.alt_of.is_empty() || has("alt-of") {
        Kind::Alt
    } else if gloss.is_empty() {
        return None;
    } else {
        Kind::Lemma
    };
    let regional = s.tags.iter().any(|t| REGION_TAGS.contains(&t.as_str()));
    let vulgar = has("vulgar") || has("slur") || (has("offensive") && !has("mildly"));
    let lower = gloss.to_lowercase();
    Some(Sense {
        kind,
        gloss,
        regional,
        spain: has("Spain"),
        marginal: regional || vulgar || s.tags.iter().any(|t| MARGINAL_TAGS.contains(&t.as_str())),
        vulgar,
        letter: lower.contains("name of the")
            || lower.contains("script letter")
            || lower.contains("greek letter")
            || lower.contains("note of the scale")
            || lower.starts_with("name of the letter"),
        comparative: has("comparative") || has("superlative"),
        apocopic: has("apocopic"),
    })
}

// --- pass 1: index -----------------------------------------------------------

/// What pass 1 remembers about each written word.
#[derive(Default)]
struct WordInfo {
    /// Has a real (non-inflection) sense as a noun/verb/adjective/adverb.
    lex_lemma: bool,
    intj_lemma: bool,
    closed: bool,
    /// Has a teachable sense that isn't regional, archaic, rare or vulgar.
    mainstream: bool,
    lex_senses: u32,
    lex_regional: u32,
    spain: bool,
    teach_senses: u32,
    teach_vulgar: u32,
    /// Also a proper name ("fernando", "chile").
    name: bool,
    /// Lemmas this word is an inflection of.
    targets: Vec<String>,
}

pub struct Index {
    words: HashMap<String, WordInfo>,
}

impl Index {
    pub fn build(reader: impl BufRead) -> Index {
        let mut index = Index {
            words: HashMap::new(),
        };
        for line in reader.lines() {
            let line = line.expect("read wiktionary dump");
            if let Ok(e) = serde_json::from_str::<LiteEntry>(&line) {
                index.add(&e);
            }
        }
        index
    }

    fn add(&mut self, e: &LiteEntry) {
        if e.lang_code != "es" || e.word.is_empty() {
            return;
        }
        if e.pos == "name" {
            self.words.entry(e.word.to_lowercase()).or_default().name = true;
            return;
        }
        let Some(class) = class_of(&e.pos) else {
            return;
        };
        let info = self.words.entry(e.word.clone()).or_default();
        if class == Class::Closed {
            info.closed = true;
        }
        let keep = keep_as_word(&e.word);
        for raw in &e.senses {
            let Some(s) = sense(raw) else { continue };
            if class == Class::Lex {
                info.lex_senses += 1;
                info.lex_regional += u32::from(s.regional);
            }
            info.spain |= s.spain;
            let kind = match s.kind {
                Kind::Form if s.comparative => Kind::Lemma,
                Kind::Form | Kind::Alt if keep => Kind::Lemma,
                k => k,
            };
            let links = match kind {
                Kind::Form => &raw.form_of,
                Kind::Alt if s.apocopic => &raw.alt_of,
                _ => &Vec::new(),
            };
            match kind {
                Kind::Lemma if !s.letter => {
                    info.teach_senses += 1;
                    info.teach_vulgar += u32::from(s.vulgar);
                    info.mainstream |= !s.marginal;
                    match class {
                        Class::Lex => info.lex_lemma = true,
                        Class::Intj => info.intj_lemma = true,
                        Class::Closed => {}
                    }
                }
                Kind::Form | Kind::Alt if class == Class::Lex => {
                    for t in links {
                        if !t.word.is_empty() && !info.targets.contains(&t.word) {
                            info.targets.push(t.word.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn info(&self, w: &str) -> Option<&WordInfo> {
        self.words.get(w)
    }

    /// Has senses worth teaching, and isn't mostly a proper name that also
    /// happens to have an obscure common-noun sense ("fernando").
    fn teachable(&self, w: &str) -> bool {
        self.info(w).is_some_and(|i| {
            (i.lex_lemma || i.intj_lemma || i.closed) && !(i.name && !i.mainstream && !i.closed)
        })
    }

    fn mainstream(&self, w: &str) -> bool {
        self.info(w).is_some_and(|i| i.closed || i.mainstream)
    }

    /// Lemmas `w` inflects, following chains like tenidas → tenido → tener.
    fn targets(&self, w: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_targets(w, 0, &mut out);
        out.sort();
        out.dedup();
        out
    }

    fn collect_targets(&self, w: &str, depth: u8, out: &mut Vec<String>) {
        let Some(info) = self.info(w) else { return };
        for t in &info.targets {
            if self.info(t).is_some_and(|i| i.lex_lemma) {
                out.push(t.clone());
            } else if depth < 2 {
                self.collect_targets(t, depth + 1, out);
            }
        }
    }

    /// Which lemma(s) a written form from the frequency list counts towards.
    fn candidates(&self, form: &str) -> Vec<String> {
        if let Some((_, lemma)) = ABBREVIATIONS.iter().find(|(a, _)| *a == form) {
            return vec![lemma.to_string()];
        }
        if !is_word(form) || FILLERS.contains(&form) {
            return Vec::new();
        }
        let targets = self.targets(form);
        let core: Vec<String> = targets
            .iter()
            .filter(|t| CORE_VERBS.contains(&t.as_str()))
            .cloned()
            .collect();
        if !core.is_empty() {
            core
        } else if self.teachable(form) && self.mainstream(form) && !prefer_inflection(form) {
            vec![form.to_string()]
        } else if !targets.is_empty() {
            targets
        } else if self.teachable(form) {
            vec![form.to_string()]
        } else {
            Vec::new()
        }
    }

    /// Lemmas ranked by how often their forms occur. A form that could belong
    /// to several lemmas is split in proportion to each one's unambiguous count.
    pub fn rank(&self, freq: &[(String, u64)]) -> Vec<(String, f64)> {
        let resolved: Vec<(Vec<String>, f64)> = freq
            .iter()
            .map(|(form, count)| (self.candidates(form), *count as f64))
            .filter(|(c, _)| !c.is_empty())
            .collect();
        let mut sure: HashMap<&str, f64> = HashMap::new();
        for (cands, count) in &resolved {
            if let [only] = cands.as_slice() {
                *sure.entry(only).or_default() += count;
            }
        }
        let mut credit: HashMap<String, f64> =
            sure.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        for (cands, count) in resolved.iter().filter(|(c, _)| c.len() > 1) {
            let weight = |w: &String| sure.get(w.as_str()).copied().unwrap_or(0.0) + 1.0;
            let total: f64 = cands.iter().map(weight).sum();
            for w in cands {
                *credit.entry(w.clone()).or_default() += count * weight(w) / total;
            }
        }
        let mut ranked: Vec<(String, f64)> = credit.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked
    }

    fn region(&self, w: &str) -> Option<&'static str> {
        let i = self.info(w)?;
        (!i.closed && i.lex_senses > 0 && i.lex_regional == i.lex_senses && !i.spain)
            .then_some("latam")
    }

    fn register(&self, w: &str) -> Option<&'static str> {
        let i = self.info(w)?;
        (i.teach_senses > 0 && i.teach_vulgar == i.teach_senses).then_some("vulgar")
    }

    /// Every written form pass 1 saw as an inflection of one of `lemmas`.
    fn inflections_of(&self, lemmas: &HashSet<String>) -> HashMap<String, Vec<String>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for form in self.words.keys() {
            if !is_word(form) {
                continue;
            }
            for t in self.targets(form) {
                if lemmas.contains(&t) {
                    out.entry(t).or_default().push(form.clone());
                }
            }
        }
        out
    }
}

fn is_word(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || "áéíóúüñ".contains(c))
}

// --- pass 2: details ---------------------------------------------------------

pub struct Conjugation {
    pub tense: &'static str,
    pub person: &'static str,
    pub form: String,
}

pub struct Lemma {
    pub lemma: String,
    pub pos: String,
    pub gloss: String,
    /// "latam" when every sense is labelled as used outside Spain.
    pub region: Option<&'static str>,
    /// "vulgar" when every sense is vulgar or a slur.
    pub register: Option<&'static str>,
    /// (form, tag): tag is "" for the lemma or an untagged inflection,
    /// "plural"/"feminine plural"/…, "participio", or "tense|person" for verbs.
    pub forms: Vec<(String, String)>,
    pub conjugations: Vec<Conjugation>,
}

/// Re-reads the dump and builds a `Lemma` for each of `wanted` that has a gloss.
pub fn details(
    reader: impl BufRead,
    wanted: &HashSet<String>,
    index: &Index,
    overrides: &HashMap<String, String>,
) -> HashMap<String, Lemma> {
    let mut entries: HashMap<String, Vec<FullEntry>> = HashMap::new();
    for line in reader.lines() {
        let line = line.expect("read wiktionary dump");
        let Ok(e) = serde_json::from_str::<FullEntry>(&line) else {
            continue;
        };
        if e.lang_code == "es" && class_of(&e.pos).is_some() && wanted.contains(&e.word) {
            entries.entry(e.word.clone()).or_default().push(e);
        }
    }
    let mut inflections = index.inflections_of(wanted);
    entries
        .into_iter()
        .filter_map(|(word, es)| {
            let extra = inflections.remove(&word).unwrap_or_default();
            build_lemma(&word, &es, index, overrides, extra)
        })
        .map(|l| (l.lemma.clone(), l))
        .collect()
}

fn build_lemma(
    word: &str,
    entries: &[FullEntry],
    index: &Index,
    overrides: &HashMap<String, String>,
    inflections: Vec<String>,
) -> Option<Lemma> {
    let primary = primary(word, entries);
    let gloss = match overrides.get(word) {
        Some(g) => g.clone(),
        None => join_glosses(&primary.as_ref()?.1),
    };
    let pos = primary
        .map(|(e, _)| &e.pos)
        .or(entries.first().map(|e| &e.pos))
        .cloned()
        .unwrap_or_default();
    let mut forms: Vec<(String, String)> = vec![(word.to_string(), String::new())];
    let mut conjugations: Vec<Conjugation> = Vec::new();
    // one table per lemma: later verb entries are usually rare homographs
    let mut table_done = false;
    for e in entries {
        let is_verb = e.pos == "verb";
        for f in &e.forms {
            if f.form.is_empty() || f.form == "-" || f.form.contains(' ') {
                continue;
            }
            let from_table = f.source.as_deref() == Some("conjugation");
            if is_verb && from_table {
                if table_done {
                    continue;
                }
                if let Some((tense, person)) = conj_slot(&f.tags) {
                    if !conjugations
                        .iter()
                        .any(|c| c.tense == tense && c.person == person)
                    {
                        conjugations.push(Conjugation {
                            tense,
                            person,
                            form: f.form.clone(),
                        });
                        forms.push((f.form.clone(), format!("{tense}|{person}")));
                    }
                } else if f.tags.iter().any(|t| t == "participle") {
                    forms.push((f.form.clone(), "participio".into()));
                }
            } else if !is_verb && !from_table {
                if let Some(tag) = inflection_tag(&f.tags) {
                    forms.push((f.form.clone(), tag));
                }
            }
        }
        table_done |= !conjugations.is_empty();
    }
    for f in inflections {
        if !forms.iter().any(|(x, _)| *x == f) {
            forms.push((f, String::new()));
        }
    }
    forms.sort();
    forms.dedup();
    Some(Lemma {
        lemma: word.to_string(),
        pos,
        gloss,
        region: index.region(word),
        register: index.register(word),
        forms,
        conjugations,
    })
}

/// Maps a Wiktionary conjugation-table cell to the app's (tense, person).
/// Voseo, -se subjunctive, future subjunctive, negative imperative and
/// pronoun-attached forms are left out.
fn conj_slot(tags: &[String]) -> Option<(&'static str, &'static str)> {
    let has = |t: &str| tags.iter().any(|x| x == t);
    const SKIP: [&str; 9] = [
        "vos-form",
        "combined-form",
        "imperfect-se",
        "negative",
        "table-tags",
        "inflection-template",
        "class",
        "infinitive",
        "second-person-semantically",
    ];
    if SKIP.iter().any(|t| has(t)) {
        return None;
    }
    if has("gerund") {
        return Some(("gerundio", ""));
    }
    if has("participle") {
        return (has("masculine") && has("singular")).then_some(("participio", ""));
    }
    let tense = if has("imperative") {
        "imperativo"
    } else if has("subjunctive") {
        if has("present") {
            "subjuntivo presente"
        } else if has("imperfect") {
            "subjuntivo imperfecto"
        } else {
            return None;
        }
    } else if has("conditional") {
        "condicional"
    } else if has("present") {
        "presente"
    } else if has("preterite") {
        "pretérito"
    } else if has("imperfect") {
        "imperfecto"
    } else if has("future") {
        "futuro"
    } else {
        return None;
    };
    let plural = has("plural");
    let person = match (
        has("first-person"),
        has("second-person"),
        has("third-person"),
    ) {
        (true, _, _) => 0,
        (_, true, _) => 1,
        (_, _, true) => 2,
        _ => return None,
    } + if plural { 3 } else { 0 };
    Some((tense, PERSONS[person]))
}

/// "plural", "feminine", "feminine plural", … for noun/adjective forms.
fn inflection_tag(tags: &[String]) -> Option<String> {
    const ALLOWED: [&str; 4] = ["plural", "singular", "masculine", "feminine"];
    if tags.is_empty() || tags.iter().any(|t| !ALLOWED.contains(&t.as_str())) {
        return None;
    }
    let has = |t: &str| tags.iter().any(|x| x == t);
    let label = [
        has("feminine").then_some("feminine"),
        has("masculine").then_some("masculine"),
        has("plural").then_some("plural"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    (!label.is_empty()).then_some(label)
}

/// The entry that defines the word for a learner (the first, in page order,
/// with a usable gloss) and its glosses. Skips letter names ("de" is also the
/// letter D) and senses that only restate the word ("mi" the note).
fn primary<'a>(word: &str, entries: &'a [FullEntry]) -> Option<(&'a FullEntry, Vec<String>)> {
    [false, true].into_iter().find_map(|allow_marginal| {
        entries
            .iter()
            .map(|e| (e, entry_glosses(word, e, allow_marginal)))
            .find(|(_, g)| !g.is_empty())
    })
}

fn entry_glosses(word: &str, e: &FullEntry, allow_marginal: bool) -> Vec<String> {
    let closed = class_of(&e.pos) == Some(Class::Closed);
    let mut out: Vec<String> = Vec::new();
    for s in e.senses.iter().filter_map(sense) {
        if s.letter || (s.marginal && !allow_marginal) {
            continue;
        }
        let g = match s.kind {
            Kind::Lemma => clean_gloss(s.gloss),
            // function words and comparatives are defined as forms: "accusative
            // of yo: me", "comparative degree of malo: worse"
            _ if closed || s.comparative || keep_as_word(word) => clean_derived(s.gloss),
            _ => None,
        };
        let Some(g) = g else { continue };
        let same = |a: &str, b: &str| {
            a.trim_end_matches('!')
                .eq_ignore_ascii_case(b.trim_end_matches('!'))
        };
        if same(&g, word) || descriptive(&g) || out.iter().any(|o| same(o, &g)) {
            continue;
        }
        out.push(g);
    }
    out
}

/// Usage notes rather than translations: "Used to express…", "Senses relating to…".
fn descriptive(g: &str) -> bool {
    const NOTES: [&str; 12] = [
        "used ",
        "senses ",
        "denotes ",
        "indicates ",
        "expresses ",
        "said ",
        "dependent ",
        "substitutes ",
        "points out ",
        "best translated ",
        "synonym of ",
        "only used ",
    ];
    let lower = g.to_lowercase();
    NOTES.iter().any(|n| lower.starts_with(n))
        || lower.contains(" senses")
        || (g.chars().next().is_some_and(char::is_uppercase) && g.split_whitespace().count() >= 3)
}

/// Up to three senses within 48 characters: short enough for an answer button.
fn join_glosses(glosses: &[String]) -> String {
    let mut chosen: Vec<&str> = Vec::new();
    let mut len = 0;
    for g in glosses {
        let n = g.chars().count();
        if !chosen.is_empty() && len + 2 + n > 48 {
            break;
        }
        len += if chosen.is_empty() { n } else { n + 2 };
        chosen.push(g);
        if chosen.len() == 3 {
            break;
        }
    }
    chosen.join("; ")
}

/// First short segment of a Wiktionary gloss, without parentheticals.
fn clean_gloss(raw: &str) -> Option<String> {
    let mut s = String::new();
    let mut depth = 0u32;
    for c in raw.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '“' | '”' | '"' => {}
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    let s = match s.split_once(':') {
        Some((_, after)) if !after.trim().is_empty() => after.to_string(),
        _ => s,
    };
    let first = s.split(';').next()?.trim();
    let first = first.strip_prefix("translated as ").unwrap_or(first);
    let first = first
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(", ");
    let first = first.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = first.trim_end_matches(['.', ',', ':', ';', ' ']);
    if first.is_empty() {
        return None;
    }
    Some(shorten(first, 40))
}

/// The English inside a "form of" gloss: "accusative of yo: me" → "me",
/// "apocopic form of mío, my" → "my", "masculine plural of el (“the”)" → "the".
fn clean_derived(raw: &str) -> Option<String> {
    if let (Some(a), Some(b)) = (raw.find('“'), raw.find('”')) {
        if a < b {
            return clean_gloss(&raw[a + '“'.len_utf8()..b]);
        }
    }
    if let Some(i) = raw.rfind(':') {
        return clean_gloss(&raw[i + 1..]);
    }
    let (_, rest) = raw.split_once(" of ")?;
    let (_, after) = rest.split_once([',', ';'])?;
    clean_gloss(after)
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    match cut.rfind(", ") {
        Some(i) if i > 0 => cut[..i].to_string(),
        _ => match cut.rfind(' ') {
            Some(i) if i > 0 => cut[..i].to_string(),
            _ => cut,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(lines: &[&str]) -> Index {
        let jsonl: Vec<String> = lines.iter().map(|l| l.replace('\n', "")).collect();
        Index::build(jsonl.join("\n").as_bytes())
    }

    const TENER: &str = r#"{"word":"tener","pos":"verb","lang_code":"es","senses":[{"glosses":["to have; to possess"],"tags":["transitive"]}],"forms":[
        {"form":"irregular","tags":["table-tags"],"source":"conjugation"},
        {"form":"teniendo","tags":["gerund"],"source":"conjugation"},
        {"form":"tenido","tags":["masculine","participle","past","singular"],"source":"conjugation"},
        {"form":"tenidas","tags":["feminine","participle","past","plural"],"source":"conjugation"},
        {"form":"tengo","tags":["first-person","indicative","present","singular"],"source":"conjugation"},
        {"form":"tenés","tags":["indicative","informal","present","second-person","singular","vos-form"],"source":"conjugation"},
        {"form":"tienes","tags":["indicative","informal","present","second-person","singular"],"source":"conjugation"},
        {"form":"tenéis","tags":["indicative","plural","present","second-person"],"source":"conjugation"},
        {"form":"tuve","tags":["first-person","indicative","preterite","singular"],"source":"conjugation"},
        {"form":"tuviese","tags":["first-person","imperfect","imperfect-se","singular","subjunctive"],"source":"conjugation"},
        {"form":"tuviera","tags":["first-person","imperfect","singular","subjunctive"],"source":"conjugation"},
        {"form":"tendría","tags":["conditional","first-person","indicative","singular"],"source":"conjugation"},
        {"form":"ten","tags":["imperative","informal","second-person","singular"],"source":"conjugation"},
        {"form":"tenga","tags":["formal","imperative","second-person-semantically","singular","third-person"],"source":"conjugation"},
        {"form":"tengas","tags":["imperative","negative","second-person","singular"],"source":"conjugation"},
        {"form":"tenerlo","tags":["accusative","combined-form","infinitive"],"source":"conjugation"}]}"#;
    const TENGO: &str = r#"{"word":"tengo","pos":"verb","lang_code":"es","senses":[{"glosses":["first-person singular present indicative of tener"],"tags":["form-of"],"form_of":[{"word":"tener"}]}]}"#;
    const CASA_NOUN: &str = r#"{"word":"casa","pos":"noun","lang_code":"es","senses":[{"glosses":["house"],"tags":["feminine"]}],"forms":[{"form":"casas","tags":["plural"]}]}"#;
    const CASA_VERB: &str = r#"{"word":"casa","pos":"verb","lang_code":"es","senses":[{"glosses":["inflection of casar:"],"tags":["form-of"],"form_of":[{"word":"casar"}]}]}"#;
    const CASAR: &str =
        r#"{"word":"casar","pos":"verb","lang_code":"es","senses":[{"glosses":["to marry"]}]}"#;
    const SALE_INTJ: &str = r#"{"word":"sale","pos":"intj","lang_code":"es","senses":[{"glosses":["ok"],"tags":["Mexico"]}]}"#;
    const SALE_VERB: &str = r#"{"word":"sale","pos":"verb","lang_code":"es","senses":[{"glosses":["third-person singular present indicative of salir"],"tags":["form-of"],"form_of":[{"word":"salir"}]}]}"#;
    const SALIR: &str = r#"{"word":"salir","pos":"verb","lang_code":"es","senses":[{"glosses":["to leave, to go out"]}]}"#;
    const ME: &str = r#"{"word":"me","pos":"pron","lang_code":"es","senses":[{"glosses":["accusative of yo: me"],"tags":["form-of"],"form_of":[{"word":"yo"}]}]}"#;
    const YO: &str = r#"{"word":"yo","pos":"pron","lang_code":"es","senses":[{"glosses":["I"]}]}"#;
    const ERA_NOUN: &str =
        r#"{"word":"era","pos":"noun","lang_code":"es","senses":[{"glosses":["era, age"]}]}"#;
    const ERA_VERB: &str = r#"{"word":"era","pos":"verb","lang_code":"es","senses":[{"glosses":["first/third-person singular imperfect indicative of ser"],"tags":["form-of"],"form_of":[{"word":"ser"}]}]}"#;
    const SER: &str =
        r#"{"word":"ser","pos":"verb","lang_code":"es","senses":[{"glosses":["to be"]}]}"#;

    fn ranked(idx: &Index, freq: &[(&str, u64)]) -> Vec<String> {
        let freq: Vec<(String, u64)> = freq.iter().map(|(f, c)| (f.to_string(), *c)).collect();
        idx.rank(&freq).into_iter().map(|(w, _)| w).collect()
    }

    #[test]
    fn inflections_count_towards_their_lemma() {
        let idx = index(&[TENER, TENGO]);
        assert_eq!(ranked(&idx, &[("tengo", 100), ("tener", 5)]), ["tener"]);
    }

    #[test]
    fn a_mainstream_word_keeps_its_own_count() {
        let idx = index(&[CASA_NOUN, CASA_VERB, CASAR]);
        let r = ranked(&idx, &[("casa", 100), ("casar", 3)]);
        assert_eq!(r, ["casa", "casar"]);
    }

    #[test]
    fn a_regional_homograph_does_not_steal_the_verb_form() {
        let idx = index(&[SALE_INTJ, SALE_VERB, SALIR]);
        assert_eq!(ranked(&idx, &[("sale", 100)]), ["salir"]);
    }

    #[test]
    fn curated_forms_go_to_their_verb() {
        let idx = index(&[
            r#"{"word":"pienso","pos":"noun","lang_code":"es","senses":[{"glosses":["animal feed"]}]}"#,
            r#"{"word":"pienso","pos":"verb","lang_code":"es","senses":[{"glosses":["first-person singular present indicative of pensar"],"tags":["form-of"],"form_of":[{"word":"pensar"}]}]}"#,
            r#"{"word":"pensar","pos":"verb","lang_code":"es","senses":[{"glosses":["to think"]}]}"#,
        ]);
        assert_eq!(ranked(&idx, &[("pienso", 100)]), ["pensar"]);
    }

    #[test]
    fn comparatives_stay_words_and_apocopic_forms_fold() {
        let idx = index(&[
            r#"{"word":"malo","pos":"adj","lang_code":"es","senses":[{"glosses":["bad"]}]}"#,
            r#"{"word":"peor","pos":"adj","lang_code":"es","senses":[{"glosses":["comparative degree of malo: worse"],"tags":["comparative","form-of"],"form_of":[{"word":"malo"}]}]}"#,
            r#"{"word":"bueno","pos":"adj","lang_code":"es","senses":[{"glosses":["good"]}]}"#,
            r#"{"word":"buen","pos":"adj","lang_code":"es","senses":[{"glosses":["apocopic form of bueno"],"tags":["alt-of","apocopic"],"alt_of":[{"word":"bueno"}]}]}"#,
            r#"{"word":"mucho","pos":"adj","lang_code":"es","senses":[{"glosses":["much"]}]}"#,
            r#"{"word":"muy","pos":"adv","lang_code":"es","senses":[{"glosses":["apocopic form of mucho; very"],"tags":["alt-of","apocopic"],"alt_of":[{"word":"mucho"}]}]}"#,
        ]);
        let r = ranked(
            &idx,
            &[
                ("peor", 50),
                ("malo", 40),
                ("buen", 30),
                ("muy", 20),
                ("bueno", 5),
                ("mucho", 1),
            ],
        );
        assert_eq!(r, ["peor", "malo", "bueno", "muy", "mucho"]);
        let peor = full(&[
            r#"{"word":"peor","pos":"adj","senses":[{"glosses":["comparative degree of malo: worse"],"tags":["comparative","form-of"],"form_of":[{"word":"malo"}]}]}"#,
        ]);
        assert_eq!(join_glosses(&primary("peor", &peor).unwrap().1), "worse");
        let muy = full(&[
            r#"{"word":"muy","pos":"adv","senses":[{"glosses":["apocopic form of mucho; very"],"tags":["alt-of","apocopic"],"alt_of":[{"word":"mucho"}]}]}"#,
        ]);
        assert_eq!(join_glosses(&primary("muy", &muy).unwrap().1), "very");
    }

    #[test]
    fn function_words_are_not_folded_into_other_words() {
        let idx = index(&[ME, YO]);
        assert_eq!(ranked(&idx, &[("me", 100), ("yo", 50)]), ["me", "yo"]);
    }

    #[test]
    fn core_verbs_win_over_rare_homographs() {
        let idx = index(&[ERA_NOUN, ERA_VERB, SER]);
        assert_eq!(ranked(&idx, &[("era", 100)]), ["ser"]);
    }

    #[test]
    fn abbreviations_and_fillers() {
        let idx = index(&[
            r#"{"word":"señor","pos":"noun","lang_code":"es","senses":[{"glosses":["sir"]}]}"#,
        ]);
        assert_eq!(ranked(&idx, &[("sr.", 10), ("oh", 99)]), ["señor"]);
    }

    #[test]
    fn conjugation_table_maps_to_app_slots_without_voseo() {
        let idx = index(&[TENER]);
        let wanted: HashSet<String> = ["tener".to_string()].into();
        let lemmas = details(
            TENER.replace('\n', "").as_bytes(),
            &wanted,
            &idx,
            &HashMap::new(),
        );
        let tener = &lemmas["tener"];
        let slot = |t: &str, p: &str| {
            tener
                .conjugations
                .iter()
                .find(|c| c.tense == t && c.person == p)
                .map(|c| c.form.as_str())
        };
        assert_eq!(slot("presente", "yo"), Some("tengo"));
        assert_eq!(slot("presente", "tú"), Some("tienes"));
        assert_eq!(slot("presente", "vosotros"), Some("tenéis"));
        assert_eq!(slot("pretérito", "yo"), Some("tuve"));
        assert_eq!(slot("subjuntivo imperfecto", "yo"), Some("tuviera"));
        assert_eq!(slot("condicional", "yo"), Some("tendría"));
        assert_eq!(slot("imperativo", "tú"), Some("ten"));
        assert_eq!(slot("gerundio", ""), Some("teniendo"));
        assert_eq!(slot("participio", ""), Some("tenido"));
        assert!(tener
            .conjugations
            .iter()
            .all(|c| !["tenés", "tuviese", "tengas", "tenerlo"].contains(&c.form.as_str())));
        assert!(tener
            .forms
            .contains(&("tenidas".into(), "participio".into())));
        assert_eq!(tener.gloss, "to have");
    }

    #[test]
    fn nouns_get_inflections_and_no_conjugations() {
        let idx = index(&[CASA_NOUN]);
        let wanted: HashSet<String> = ["casa".to_string()].into();
        let casa = &details(CASA_NOUN.as_bytes(), &wanted, &idx, &HashMap::new())["casa"];
        assert!(casa.conjugations.is_empty());
        assert!(casa.forms.contains(&("casas".into(), "plural".into())));
        assert_eq!(casa.pos, "noun");
    }

    #[test]
    fn region_and_register_flags() {
        let idx = index(&[
            r#"{"word":"computadora","pos":"noun","lang_code":"es","senses":[{"glosses":["computer"],"tags":["Mexico","Peru"]}]}"#,
            r#"{"word":"ordenador","pos":"noun","lang_code":"es","senses":[{"glosses":["computer"],"tags":["Spain"]}]}"#,
            r#"{"word":"joder","pos":"verb","lang_code":"es","senses":[{"glosses":["to annoy"],"tags":["vulgar"]}]}"#,
            r#"{"word":"tonto","pos":"adj","lang_code":"es","senses":[{"glosses":["silly"],"tags":["derogatory","mildly","offensive"]}]}"#,
        ]);
        assert_eq!(idx.region("computadora"), Some("latam"));
        assert_eq!(idx.region("ordenador"), None);
        assert_eq!(idx.register("joder"), Some("vulgar"));
        assert_eq!(idx.register("tonto"), None);
    }

    fn full(lines: &[&str]) -> Vec<FullEntry> {
        lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn proper_names_with_only_obscure_senses_are_dropped() {
        let idx = index(&[
            r#"{"word":"Fernando","pos":"name","lang_code":"es","senses":[{"glosses":["a male given name"]}]}"#,
            r#"{"word":"fernando","pos":"noun","lang_code":"es","senses":[{"glosses":["a kind of bread"],"tags":["Colombia"]}]}"#,
            r#"{"word":"Rosa","pos":"name","lang_code":"es","senses":[{"glosses":["a female given name"]}]}"#,
            r#"{"word":"rosa","pos":"noun","lang_code":"es","senses":[{"glosses":["rose"]}]}"#,
        ]);
        assert_eq!(ranked(&idx, &[("fernando", 90), ("rosa", 10)]), ["rosa"]);
    }

    #[test]
    fn primary_entry_skips_letter_names_self_glosses_and_usage_notes() {
        let de = full(&[
            r#"{"word":"de","pos":"noun","senses":[{"glosses":["The name of the Latin script letter D/d."]}]}"#,
            r#"{"word":"de","pos":"prep","senses":[{"glosses":["of; 's"]},{"glosses":["from (with the source of)"]}]}"#,
        ]);
        let (e, g) = primary("de", &de).unwrap();
        assert_eq!(
            (e.pos.as_str(), join_glosses(&g).as_str()),
            ("prep", "of; from")
        );

        let mi = full(&[
            r#"{"word":"mi","pos":"noun","senses":[{"glosses":["mi"]}]}"#,
            r#"{"word":"mi","pos":"det","senses":[{"glosses":["apocopic form of mío, my"],"tags":["alt-of"],"alt_of":[{"word":"mío"}]}]}"#,
        ]);
        let (e, g) = primary("mi", &mi).unwrap();
        assert_eq!((e.pos.as_str(), join_glosses(&g).as_str()), ("det", "my"));

        let venir = full(&[
            r#"{"word":"venir","pos":"verb","senses":[{"glosses":["Senses relating to literal movement"]},{"glosses":["Senses relating to literal movement","to come"]},{"glosses":["used to express something"]}]}"#,
        ]);
        assert_eq!(
            join_glosses(&primary("venir", &venir).unwrap().1),
            "to come"
        );
    }

    #[test]
    fn gloss_cleaning() {
        assert_eq!(
            clean_gloss("translated as to like").as_deref(),
            Some("to like")
        );
        assert_eq!(
            clean_gloss("to do, perform, execute, carry out").as_deref(),
            Some("to do, perform, execute")
        );
        assert_eq!(
            clean_gloss("yes, yeah, (used to respond)").as_deref(),
            Some("yes, yeah")
        );
        assert_eq!(
            clean_gloss("from (with the source or provenance of or at)").as_deref(),
            Some("from")
        );
        assert_eq!(
            clean_gloss("of; 's; used after the thing owned").as_deref(),
            Some("of")
        );
        assert_eq!(
            clean_gloss("A reflexive pronoun: oneself, himself").as_deref(),
            Some("oneself, himself")
        );
        assert_eq!(clean_derived("accusative of yo: me").as_deref(), Some("me"));
        assert_eq!(
            clean_derived("apocopic form of mío, my").as_deref(),
            Some("my")
        );
        assert_eq!(
            clean_derived("masculine plural of el (“the”)").as_deref(),
            Some("the")
        );
        assert_eq!(
            clean_derived("neuter singular of ése; that").as_deref(),
            Some("that")
        );
    }
}
