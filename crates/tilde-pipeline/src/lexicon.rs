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
use std::sync::OnceLock;

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

/// A hand-kept word list from `curation/`: one entry per line, `#` comments.
/// Read once, because pass 1 asks about every entry of the dump.
struct List {
    text: &'static str,
    words: OnceLock<HashSet<&'static str>>,
}

impl List {
    const fn new(text: &'static str) -> List {
        List {
            text,
            words: OnceLock::new(),
        }
    }

    fn words(&self) -> &HashSet<&'static str> {
        self.words.get_or_init(|| {
            self.text
                .lines()
                .filter(|l| !l.starts_with('#'))
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect()
        })
    }
}

static LEMMAS: List = List::new(include_str!("../curation/lemmas.txt"));
static INFLECTIONS: List = List::new(include_str!("../curation/inflections.txt"));
static EXCLUDE: List = List::new(include_str!("../curation/exclude.txt"));
static VULGAR: List = List::new(include_str!("../curation/vulgar.txt"));
static NOT_REGIONAL: List = List::new(include_str!("../curation/not_regional.txt"));

/// Words kept as their own entry although Wiktionary defines them as a form.
fn keep_as_word(word: &str) -> bool {
    LEMMAS.words().contains(word)
}

/// Forms credited to what they inflect rather than to their own rare sense.
fn prefer_inflection(form: &str) -> bool {
    INFLECTIONS.words().contains(form)
}

/// Written forms that aren't Spanish words (names, English, misspellings,
/// letters): they count for nothing, are never a lemma and are no form of one.
pub fn excluded(form: &str) -> bool {
    EXCLUDE.words().contains(form)
}

/// Words whose everyday meaning is vulgar although Wiktionary also lists an
/// innocent sense.
fn vulgar_listed(word: &str) -> bool {
    VULGAR.words().contains(word)
}

/// Words standard in Spain whose senses Wiktionary labels Latin-American.
fn not_regional(word: &str) -> bool {
    NOT_REGIONAL.words().contains(word)
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

/// Wiktionary region labels outside Spain. A sense that also names Spain is
/// Peninsular, whatever else it names (see `sense`).
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
    /// Slang or an insult: only the gloss when there's nothing plainer, so
    /// "perro" is "dog", not "dog; asshole". (Colloquial senses stay: in
    /// Spain "tío" is "guy" as much as "uncle".)
    informal: bool,
    letter: bool,
    /// "comparative degree of malo: worse": a word of its own for learners.
    comparative: bool,
    /// "apocopic form of bueno": folded into the full form.
    apocopic: bool,
}

fn sense<'a>(s: &'a RawSense, word: &str) -> Option<Sense<'a>> {
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
    // "patata" is tagged Spain and Philippines: used in Spain, so not regional
    let regional = !has("Spain")
        && !not_regional(word)
        && s.tags.iter().any(|t| REGION_TAGS.contains(&t.as_str()));
    let vulgar = has("vulgar") || has("slur") || (has("offensive") && !has("mildly"));
    let lower = gloss.to_lowercase();
    Some(Sense {
        kind,
        gloss,
        regional,
        spain: has("Spain"),
        marginal: regional || vulgar || s.tags.iter().any(|t| MARGINAL_TAGS.contains(&t.as_str())),
        vulgar,
        informal: ["slang", "derogatory", "pejorative", "offensive"]
            .iter()
            .any(|t| has(t)),
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
            let Some(s) = sense(raw, &e.word) else {
                continue;
            };
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
            let lemma = self.info(t).is_some_and(|i| i.lex_lemma);
            // A form listed in inflections.txt is no word of its own, so what
            // points at it belongs to what it inflects: "robada" → "robado"
            // → "robar". Otherwise "robado" would live on as a word through
            // its feminine and plurals.
            let through = !lemma || prefer_inflection(t);
            let found = out.len();
            if through && depth < 2 {
                self.collect_targets(t, depth + 1, out);
            }
            // a listed form that inflects nothing we know stays the word
            if lemma && (!through || out.len() == found) {
                out.push(t.clone());
            }
        }
    }

    /// Which lemma(s) a written form from the frequency list counts towards.
    fn candidates(&self, form: &str) -> Vec<String> {
        if let Some((_, lemma)) = ABBREVIATIONS.iter().find(|(a, _)| *a == form) {
            return vec![lemma.to_string()];
        }
        if !is_word(form) || FILLERS.contains(&form) || excluded(form) {
            return Vec::new();
        }
        // not to an excluded lemma either: "duele" is doler, never "dolar"
        let targets: Vec<String> = self
            .targets(form)
            .into_iter()
            .filter(|t| !excluded(t))
            .collect();
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
    /// A Spanish definition, from the Spanish Wiktionary (see definiciones.rs).
    pub gloss_es: Option<String>,
    /// "latam" when every sense is labelled as used outside Spain.
    pub region: Option<&'static str>,
    /// "vulgar" when what we'd teach is vulgar, or the word is listed in
    /// curation/vulgar.txt.
    pub register: Option<&'static str>,
    /// (form, tag): tag is "" for the lemma or an untagged inflection,
    /// "plural"/"feminine plural"/…, "participio", or "tense|person" for verbs.
    /// Without the forms listed in curation/exclude.txt, and without the
    /// gendered forms of a noun, pronoun or article whose counterpart is a
    /// word of its own (see `drop_counterparts`).
    pub forms: Vec<(String, String)>,
    pub conjugations: Vec<Conjugation>,
    /// The forms that are only this verb's *vos* forms ("tenés", "tené"): in
    /// no other cell of its table. They are in `forms` too, so a search finds
    /// the verb, but a sentence written with one isn't Castilian (see
    /// `sentences::Linker::voseo`). A name spelled like one ("Tomás") is no
    /// form at all once it is listed in curation/exclude.txt.
    pub voseo: Vec<String>,
}

/// A hand-written gloss, and part of speech when Wiktionary's first entry
/// isn't the one the gloss describes.
#[derive(Debug, PartialEq)]
pub struct Override {
    pub gloss: String,
    pub pos: Option<String>,
}

/// Reads `curation/glosses.tsv`: `lemma<TAB>gloss` or `lemma<TAB>gloss<TAB>pos`
/// lines; `#` comments and blank lines are skipped. Anything else is an error
/// naming the line: a row read wrongly would teach the wrong meaning.
pub fn parse_overrides(text: &str) -> Result<HashMap<String, Override>, String> {
    let mut out = HashMap::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let at = |what: &str| format!("line {}: {what}: {line:?}", n + 1);
        let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
        let (lemma, gloss, pos) = match fields[..] {
            [lemma, gloss] => (lemma, gloss, None),
            [lemma, gloss, pos] => (lemma, gloss, Some(pos)),
            _ => return Err(at("expected lemma<TAB>gloss or lemma<TAB>gloss<TAB>pos")),
        };
        if lemma.is_empty() || gloss.is_empty() {
            return Err(at("empty lemma or gloss"));
        }
        if pos.is_some_and(|p| class_of(p).is_none()) {
            return Err(at("unknown part of speech"));
        }
        if excluded(lemma) {
            return Err(at("lemma is in exclude.txt"));
        }
        let entry = Override {
            gloss: gloss.to_string(),
            pos: pos.map(str::to_string),
        };
        if out.insert(lemma.to_string(), entry).is_some() {
            return Err(at("lemma listed twice"));
        }
    }
    Ok(out)
}

/// Re-reads the dump and builds a `Lemma` for each of `wanted` that has a gloss.
pub fn details(
    reader: impl BufRead,
    wanted: &HashSet<String>,
    index: &Index,
    overrides: &HashMap<String, Override>,
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
    overrides: &HashMap<String, Override>,
    inflections: Vec<String>,
) -> Option<Lemma> {
    let primary = primary(word, entries);
    let curated = overrides.get(word);
    let gloss = match curated {
        Some(o) => o.gloss.clone(),
        None => {
            let (entry, glosses, _) = primary.as_ref()?;
            join_glosses(glosses, entry.pos == "verb")
        }
    };
    let pos = curated
        .and_then(|o| o.pos.as_ref())
        .or(primary.as_ref().map(|(e, ..)| &e.pos))
        .or(entries.first().map(|e| &e.pos))
        .cloned()
        .unwrap_or_default();
    // a curated gloss is the curator's wording, whatever Wiktionary leads with
    let teaches_vulgar = curated.is_none() && primary.is_some_and(|(_, _, vulgar)| vulgar);
    let register = (index.register(word).is_some() || teaches_vulgar || vulgar_listed(word))
        .then_some("vulgar");
    // "militar" the adjective has a verb homograph: its table would show under
    // the adjective, and the app only drills words whose pos is verb
    let verb = pos == "verb";
    let mut forms: Vec<(String, String)> = vec![(word.to_string(), String::new())];
    let mut conjugations: Vec<Conjugation> = Vec::new();
    // one table per lemma: later verb entries are usually rare homographs
    let mut table_done = false;
    // spellings of the vos cells, and of every other cell, of all its tables
    let mut vos: HashSet<&str> = HashSet::new();
    let mut not_vos: HashSet<&str> = HashSet::new();
    // what the letter-name entries list: no forms of this word, even where
    // pass 1 took "míes" (the plural of the Greek mi) for a plural of "mi"
    let mut of_letter: HashSet<&str> = HashSet::new();
    for e in entries {
        // "es" is the plural of the letter e, not a form of "e" (and)
        if names_a_letter(e, word) {
            of_letter.extend(e.forms.iter().map(|f| f.form.as_str()));
            continue;
        }
        let is_verb = e.pos == "verb";
        for f in &e.forms {
            if f.form.is_empty() || f.form == "-" || f.form.contains(' ') {
                continue;
            }
            let from_table = f.source.as_deref() == Some("conjugation");
            if is_verb && from_table {
                let has = |t: &str| f.tags.iter().any(|x| x == t);
                if has("vos-form") {
                    vos.insert(&f.form);
                } else if !TABLE_NOTES.iter().any(|t| has(t)) {
                    not_vos.insert(&f.form);
                }
                if table_done || !verb {
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
        if !of_letter.contains(f.as_str()) && !forms.iter().any(|(x, _)| *x == f) {
            forms.push((f, String::new()));
        }
    }
    // "china" is the country far more often than the feminine of chino: a
    // listed non-word must find no word, in a search or in a sentence. The
    // table above keeps its cell ("charles"), which is still conjugated.
    forms.retain(|(f, _)| !excluded(f));
    forms.sort();
    forms.dedup();
    // "sé" is the vos imperative of ser and its tú imperative: not voseo.
    // Nor is a spelling the word has for another reason.
    let voseo: Vec<String> = forms
        .iter()
        .filter(|(f, tag)| tag.is_empty() && f != word)
        .map(|(f, _)| f.as_str())
        .filter(|f| vos.contains(f) && !not_vos.contains(f))
        .filter(|f| forms.iter().filter(|(x, _)| x == f).count() == 1)
        .map(str::to_string)
        .collect();
    Some(Lemma {
        lemma: word.to_string(),
        pos,
        gloss,
        gloss_es: None,
        region: index.region(word),
        register,
        forms,
        conjugations,
        voseo,
    })
}

/// Rows of a conjugation table that describe it rather than hold a form.
const TABLE_NOTES: [&str; 3] = ["table-tags", "inflection-template", "class"];

/// An entry all of whose senses name a letter or a note ("e", the letter):
/// its plural is no form of the word that happens to be spelled like it.
fn names_a_letter(e: &FullEntry, word: &str) -> bool {
    let mut senses = e.senses.iter().filter_map(|s| sense(s, word)).peekable();
    senses.peek().is_some() && senses.all(|s| s.letter)
}

/// A noun's, pronoun's or article's counterpart of the other gender is a word
/// of its own when it is on the list: "padre" is no form of madre, nor "él" of
/// ella. Such a word loses every form of that gender ("padres" too), so the
/// counterpart's sentences aren't its examples. Adjectives and determiners
/// keep theirs: "nueva" is a form of nuevo even where it is also listed as a
/// noun. Returns the number of form rows removed.
pub fn drop_counterparts(words: &mut [Lemma]) -> usize {
    let headwords: HashSet<String> = words.iter().map(|w| w.lemma.to_lowercase()).collect();
    let mut dropped = 0;
    for w in words.iter_mut() {
        if !matches!(w.pos.as_str(), "noun" | "pron" | "article") {
            continue;
        }
        let own = w.lemma.to_lowercase();
        let genders: Vec<&str> = ["masculine", "feminine"]
            .into_iter()
            .filter(|g| {
                w.forms.iter().any(|(f, tag)| {
                    let f = f.to_lowercase();
                    tag == g && f != own && headwords.contains(&f)
                })
            })
            .collect();
        let before = w.forms.len();
        // rows, not spellings: padre keeps "padres" the plural and loses
        // "padres" the "feminine masculine plural"
        w.forms
            .retain(|(_, tag)| !tag.split(' ').any(|t| genders.contains(&t)));
        dropped += before - w.forms.len();
    }
    dropped
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
/// with a usable gloss), its glosses, and whether any of them comes from a
/// vulgar sense. Skips letter names ("de" is also the letter D) and a sense
/// that only restates the word, unless that is what the word first means
/// ("idea", "hotel") and it isn't a function word ("mi" the note).
fn primary<'a>(word: &str, entries: &'a [FullEntry]) -> Option<(&'a FullEntry, Vec<String>, bool)> {
    let function_word = entries
        .iter()
        .any(|e| class_of(&e.pos) == Some(Class::Closed));
    [false, true].into_iter().find_map(|allow_marginal| {
        entries
            .iter()
            .map(|e| {
                let (glosses, vulgar) = entry_glosses(word, e, allow_marginal, function_word);
                (e, glosses, vulgar)
            })
            .find(|(_, g, _)| !g.is_empty())
    })
}

fn entry_glosses(
    word: &str,
    e: &FullEntry,
    allow_marginal: bool,
    function_word: bool,
) -> (Vec<String>, bool) {
    let closed = class_of(&e.pos) == Some(Class::Closed);
    let senses: Vec<Sense> = e
        .senses
        .iter()
        .filter_map(|s| sense(s, word))
        .filter(|s| !s.letter && (allow_marginal || !s.marginal))
        .collect();
    // a noun or adjective with no sense of its own means what it points to:
    // "moto" is a clipping of motocicleta, "maestra" the female maestro. Not
    // verbs: a participle's quoted meaning ("praised") is no word to teach.
    let pointer = matches!(e.pos.as_str(), "noun" | "adj")
        && !senses
            .iter()
            .any(|s| s.kind == Kind::Lemma && clean_gloss(s.gloss).is_some());
    let mut out: Vec<String> = Vec::new();
    let mut informal: Vec<String> = Vec::new();
    let (mut out_vulgar, mut informal_vulgar) = (false, false);
    for s in &senses {
        let g = match s.kind {
            Kind::Lemma => clean_gloss(s.gloss),
            // function words and comparatives are defined as forms: "accusative
            // of yo: me", "comparative degree of malo: worse"
            _ if closed || s.comparative || keep_as_word(word) => clean_derived(s.gloss),
            _ if pointer && !s.informal => clean_derived(s.gloss),
            _ => None,
        };
        let Some(g) = g else { continue };
        let same = |a: &str, b: &str| {
            a.trim_end_matches('!')
                .eq_ignore_ascii_case(b.trim_end_matches('!'))
        };
        // a cognate is its own translation when it leads the entry. A later
        // one is a borrowed technical sense ("mesa" in geology), and one that
        // differs in case is a name ("Romance").
        let cognate = g.trim_end_matches('!') == word.trim_end_matches('!')
            && out.is_empty()
            && informal.is_empty()
            && !function_word;
        if (same(&g, word) && !cognate)
            || descriptive(&g)
            || out.iter().chain(&informal).any(|o| same(o, &g))
        {
            continue;
        }
        if s.informal {
            informal.push(g);
            informal_vulgar |= s.vulgar;
        } else {
            out.push(g);
            out_vulgar |= s.vulgar;
        }
    }
    if out.is_empty() {
        (informal, informal_vulgar)
    } else {
        (out, out_vulgar)
    }
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
fn join_glosses(glosses: &[String], verb: bool) -> String {
    let mut chosen: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut len = 0;
    for g in glosses {
        // later senses often repeat an earlier one's words ("and; plus, and",
        // and for verbs "to start, begin; to begin"): keep only what's new,
        // and skip a sense with nothing new. Only for verbs is "to wait" the
        // same as "wait"; "to you" and "you" are different senses of "te".
        let key = |part: &str| {
            let lower = part.to_lowercase();
            match lower.strip_prefix("to ") {
                Some(bare) if verb => bare.to_string(),
                _ => lower,
            }
        };
        let mut fresh: Vec<&str> = Vec::new();
        for part in g.split(", ") {
            if !seen.contains(&key(part)) && !fresh.iter().any(|f| key(f) == key(part)) {
                fresh.push(part);
            }
        }
        if fresh.is_empty() {
            continue;
        }
        let sense = fresh.join(", ");
        let n = sense.chars().count();
        if !chosen.is_empty() && len + 2 + n > 48 {
            break;
        }
        len += if chosen.is_empty() { n } else { n + 2 };
        seen.extend(fresh.iter().map(|p| key(p)));
        chosen.push(sense);
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
    fn a_listed_form_is_no_word_for_its_own_forms_either() {
        // "robado" is in inflections.txt: an adjective in the dictionary, but
        // nothing more than the participle of robar
        let idx = index(&[
            r#"{"word":"robar","pos":"verb","lang_code":"es","senses":[{"glosses":["to steal"]}]}"#,
            r#"{"word":"robado","pos":"adj","lang_code":"es","senses":[{"glosses":["stolen"]}]}"#,
            r#"{"word":"robado","pos":"verb","lang_code":"es","senses":[{"glosses":["past participle of robar"],"tags":["form-of","participle","past"],"form_of":[{"word":"robar"}]}]}"#,
            r#"{"word":"robada","pos":"adj","lang_code":"es","senses":[{"glosses":["feminine singular of robado"],"tags":["feminine","form-of","singular"],"form_of":[{"word":"robado"}]}]}"#,
            r#"{"word":"robados","pos":"adj","lang_code":"es","senses":[{"glosses":["masculine plural of robado"],"tags":["form-of","masculine","plural"],"form_of":[{"word":"robado"}]}]}"#,
        ]);
        // its feminine and plural would otherwise keep "robado" on the list
        assert_eq!(
            idx.rank(&freq(&[
                ("robado", 100),
                ("robada", 40),
                ("robados", 30),
                ("robar", 5)
            ])),
            [("robar".to_string(), 175.0)]
        );
        // and they are stored as forms of the verb, so a search finds it
        let wanted: HashSet<String> = ["robar".to_string(), "robado".to_string()].into();
        let forms = idx.inflections_of(&wanted);
        let mut of_robar = forms["robar"].clone();
        of_robar.sort();
        assert_eq!(of_robar, ["robada", "robado", "robados"]);
        assert!(!forms.contains_key("robado"));
    }

    #[test]
    fn curated_forms_go_to_their_adjective_too() {
        // "limpia" is in inflections.txt: a noun of its own ("a clean-up"), but
        // mostly the feminine of limpio or a form of limpiar
        let idx = index(&[
            r#"{"word":"limpia","pos":"noun","lang_code":"es","senses":[{"glosses":["cleaning, clean-up"]}]}"#,
            r#"{"word":"limpia","pos":"adj","lang_code":"es","senses":[{"glosses":["feminine singular of limpio"],"tags":["feminine","form-of","singular"],"form_of":[{"word":"limpio"}]}]}"#,
            r#"{"word":"limpia","pos":"verb","lang_code":"es","senses":[{"glosses":["third-person singular present indicative of limpiar"],"tags":["form-of"],"form_of":[{"word":"limpiar"}]}]}"#,
            r#"{"word":"limpio","pos":"adj","lang_code":"es","senses":[{"glosses":["clean"]}]}"#,
            r#"{"word":"limpiar","pos":"verb","lang_code":"es","senses":[{"glosses":["to clean"]}]}"#,
        ]);
        // shared in proportion to what each is known to get on its own
        assert_eq!(
            idx.rank(&freq(&[("limpia", 100), ("limpio", 29), ("limpiar", 9)])),
            [
                ("limpio".to_string(), 29.0 + 75.0),
                ("limpiar".to_string(), 9.0 + 25.0)
            ]
        );
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
        assert_eq!(
            join_glosses(&primary("peor", &peor).unwrap().1, false),
            "worse"
        );
        let muy = full(&[
            r#"{"word":"muy","pos":"adv","senses":[{"glosses":["apocopic form of mucho; very"],"tags":["alt-of","apocopic"],"alt_of":[{"word":"mucho"}]}]}"#,
        ]);
        assert_eq!(
            join_glosses(&primary("muy", &muy).unwrap().1, false),
            "very"
        );
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
    fn vos_forms_are_marked_unless_they_are_something_else_too() {
        const TENES: &str = r#"{"word":"tenés","pos":"verb","lang_code":"es","senses":[{"glosses":["second-person singular voseo present indicative of tener"],"tags":["form-of"],"form_of":[{"word":"tener"}]}]}"#;
        // "tenés" is in the forms (pass 1 saw it), though in no slot of the table
        let tener = detail("tener", &[TENER, TENGO, TENES]).unwrap();
        assert!(tener.forms.contains(&("tenés".into(), String::new())));
        assert_eq!(tener.voseo, ["tenés"]);
        // "sé" is ser's vos imperative and its tú imperative; "sos" is only vos
        let ser = [
            r#"{"word":"ser","pos":"verb","lang_code":"es","senses":[{"glosses":["to be"]}],"forms":[
                {"form":"sos","tags":["indicative","informal","present","second-person","singular","vos-form"],"source":"conjugation"},
                {"form":"eres","tags":["indicative","informal","present","second-person","singular"],"source":"conjugation"},
                {"form":"sé","tags":["imperative","informal","second-person","singular"],"source":"conjugation"},
                {"form":"sé","tags":["imperative","informal","second-person","singular","vos-form"],"source":"conjugation"}]}"#,
            r#"{"word":"sos","pos":"verb","lang_code":"es","senses":[{"glosses":["second-person singular voseo present indicative of ser"],"tags":["form-of"],"form_of":[{"word":"ser"}]}]}"#,
        ];
        assert_eq!(detail("ser", &ser).unwrap().voseo, ["sos"]);
        // "Tomás" is the name far more often than "vos tomás": the spelling
        // is in curation/exclude.txt, so tomar has no such form at all
        let tomar = [
            r#"{"word":"tomar","pos":"verb","lang_code":"es","senses":[{"glosses":["to take"]}],"forms":[
                {"form":"tomás","tags":["indicative","informal","present","second-person","singular","vos-form"],"source":"conjugation"},
                {"form":"tomá","tags":["imperative","informal","second-person","singular","vos-form"],"source":"conjugation"}]}"#,
            r#"{"word":"tomás","pos":"verb","lang_code":"es","senses":[{"glosses":["second-person singular voseo present indicative of tomar"],"tags":["form-of"],"form_of":[{"word":"tomar"}]}]}"#,
            r#"{"word":"tomá","pos":"verb","lang_code":"es","senses":[{"glosses":["second-person singular voseo imperative of tomar"],"tags":["form-of"],"form_of":[{"word":"tomar"}]}]}"#,
            r#"{"word":"Tomás","pos":"name","lang_code":"es","senses":[{"glosses":["a male given name"]}]}"#,
        ];
        let tomar = detail("tomar", &tomar).unwrap();
        assert_eq!(tomar.voseo, ["tomá"]);
        assert!(!tomar.forms.iter().any(|(f, _)| f == "tomás"));
        // that list is the only way out: a vos form Wiktionary also knows as
        // a name ("Salvá" is a surname) is a vos form like any other
        let salvar = [
            r#"{"word":"salvar","pos":"verb","lang_code":"es","senses":[{"glosses":["to save"]}],"forms":[
                {"form":"salvá","tags":["imperative","informal","second-person","singular","vos-form"],"source":"conjugation"}]}"#,
            r#"{"word":"salvá","pos":"verb","lang_code":"es","senses":[{"glosses":["second-person singular voseo imperative of salvar"],"tags":["form-of"],"form_of":[{"word":"salvar"}]}]}"#,
            r#"{"word":"Salvá","pos":"name","lang_code":"es","senses":[{"glosses":["a surname"]}]}"#,
        ];
        assert_eq!(detail("salvar", &salvar).unwrap().voseo, ["salvá"]);
        // a noun has no vos forms, whatever its verb homograph's table says
        assert!(detail("casa", &[CASA_NOUN]).unwrap().voseo.is_empty());
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
    fn letter_name_entries_add_no_forms() {
        // "es" is the plural of the letter, and nearly always the verb
        let e = detail(
            "e",
            &[
                r#"{"word":"e","pos":"conj","lang_code":"es","senses":[{"glosses":["and (used before words that begin with an i sound)"]}]}"#,
                r#"{"word":"e","pos":"noun","lang_code":"es","senses":[{"glosses":["The name of the Latin-script letter E/e."],"tags":["feminine"]}],"forms":[{"form":"es","tags":["plural"]}]}"#,
            ],
        )
        .unwrap();
        assert_eq!(e.forms, [("e".to_string(), String::new())]);
        // "mi" is a noun too (the note), so pass 1 takes "míes", the plural
        // of the Greek letter, for its plural: still not a form of "mi"
        let mi = detail(
            "mi",
            &[
                r#"{"word":"mi","pos":"det","lang_code":"es","senses":[{"glosses":["apocopic form of mío, my"],"tags":["alt-of","apocopic"],"alt_of":[{"word":"mío"}]}],"forms":[{"form":"mis","tags":["plural"]}]}"#,
                r#"{"word":"mi","pos":"noun","lang_code":"es","senses":[{"glosses":["mu; the Greek letter Μ, μ"],"tags":["feminine"]}],"forms":[{"form":"míes","tags":["plural"]}]}"#,
                r#"{"word":"mi","pos":"noun","lang_code":"es","senses":[{"glosses":["mi"],"tags":["masculine"]}],"forms":[{"form":"mis","tags":["plural"]}]}"#,
                r#"{"word":"míes","pos":"noun","lang_code":"es","senses":[{"glosses":["plural of mi"],"tags":["form-of","plural"],"form_of":[{"word":"mi"}]}]}"#,
                r#"{"word":"mis","pos":"noun","lang_code":"es","senses":[{"glosses":["plural of mi"],"tags":["form-of","plural"],"form_of":[{"word":"mi"}]}]}"#,
            ],
        )
        .unwrap();
        assert_eq!(forms_of(&mi), [("mi", ""), ("mis", "plural")]);
        // an entry that also means something else keeps its forms
        let delta = detail(
            "delta",
            &[
                r#"{"word":"delta","pos":"noun","lang_code":"es","senses":[{"glosses":["delta (Greek letter)"]},{"glosses":["delta (landform at the mouth of a river)"]}],"forms":[{"form":"deltas","tags":["plural"]}]}"#,
            ],
        )
        .unwrap();
        assert!(delta.forms.contains(&("deltas".into(), "plural".into())));
    }

    fn listed(lemma: &str, pos: &str, forms: &[(&str, &str)]) -> Lemma {
        let mut all = vec![(lemma.to_string(), String::new())];
        all.extend(forms.iter().map(|(f, t)| (f.to_string(), t.to_string())));
        all.sort();
        Lemma {
            lemma: lemma.to_string(),
            pos: pos.to_string(),
            gloss: "gloss".to_string(),
            gloss_es: None,
            region: None,
            register: None,
            forms: all,
            conjugations: Vec::new(),
            voseo: Vec::new(),
        }
    }

    fn forms_of(l: &Lemma) -> Vec<(&str, &str)> {
        l.forms
            .iter()
            .map(|(f, t)| (f.as_str(), t.as_str()))
            .collect()
    }

    #[test]
    fn a_nouns_counterpart_is_not_its_form() {
        // the rows Wiktionary's noun tables give the two words
        let mut words = [
            listed(
                "madre",
                "noun",
                &[
                    ("madres", "plural"),
                    ("padre", "masculine"),
                    ("padres", "masculine plural"),
                ],
            ),
            listed(
                "padre",
                "noun",
                &[
                    ("madre", "feminine"),
                    ("madres", "feminine plural"),
                    ("padres", "feminine masculine plural"),
                    ("padres", "plural"),
                    ("padrecito", ""),
                ],
            ),
        ];
        assert_eq!(drop_counterparts(&mut words), 5);
        assert_eq!(forms_of(&words[0]), [("madre", ""), ("madres", "plural")]);
        // rows go, not spellings: "padres" is still padre's plural
        assert_eq!(
            forms_of(&words[1]),
            [("padre", ""), ("padrecito", ""), ("padres", "plural")]
        );
        // nothing left to drop
        assert_eq!(drop_counterparts(&mut words), 0);
    }

    #[test]
    fn pronouns_and_articles_lose_their_counterparts_too() {
        let mut words = [
            listed(
                "ella",
                "pron",
                &[
                    ("ellas", "plural"),
                    ("ellos", "masculine plural"),
                    ("él", "masculine"),
                ],
            ),
            listed(
                "él",
                "pron",
                &[
                    ("ella", "feminine"),
                    ("ellas", "feminine plural"),
                    ("ellos", "masculine plural"),
                ],
            ),
            listed(
                "el",
                "article",
                &[
                    ("la", "feminine"),
                    ("las", "feminine plural"),
                    ("los", "plural"),
                ],
            ),
            listed(
                "la",
                "article",
                &[
                    ("el", "masculine"),
                    ("las", "plural"),
                    ("los", "masculine plural"),
                ],
            ),
        ];
        assert_eq!(drop_counterparts(&mut words), 8);
        assert_eq!(forms_of(&words[0]), [("ella", ""), ("ellas", "plural")]);
        // its own gender stays, whatever the tag calls it
        assert_eq!(
            forms_of(&words[1]),
            [("ellos", "masculine plural"), ("él", "")]
        );
        assert_eq!(forms_of(&words[2]), [("el", ""), ("los", "plural")]);
        assert_eq!(forms_of(&words[3]), [("la", ""), ("las", "plural")]);
    }

    #[test]
    fn an_adjective_keeps_its_feminine_even_if_that_is_a_word() {
        // "nueva" is also listed as a noun ("news"): "una casa nueva" is
        // still nuevo, and the sentence's English says which it is
        let nuevo = [
            ("nueva", "feminine"),
            ("nuevas", "feminine plural"),
            ("nuevos", "masculine plural"),
        ];
        let este = [("esta", "feminine"), ("estas", "feminine plural")];
        let mut words = [
            listed("nuevo", "adj", &nuevo),
            listed("nueva", "noun", &[("nuevas", "plural")]),
            listed("este", "det", &este),
            listed("esta", "pron", &[]),
        ];
        assert_eq!(drop_counterparts(&mut words), 0);
        assert_eq!(
            forms_of(&words[0]),
            forms_of(&listed("nuevo", "adj", &nuevo))
        );
        assert_eq!(forms_of(&words[2]), forms_of(&listed("este", "det", &este)));
    }

    #[test]
    fn a_counterpart_not_in_the_list_is_kept() {
        let alcalde = [
            ("alcaldes", "plural"),
            ("alcaldesa", "feminine"),
            ("alcaldesas", "feminine plural"),
        ];
        let mut words = [
            listed("alcalde", "noun", &alcalde),
            // a word that is only somebody's plural triggers nothing
            listed("alcaldesas", "noun", &[]),
        ];
        assert_eq!(drop_counterparts(&mut words), 0);
        assert_eq!(
            forms_of(&words[0]),
            forms_of(&listed("alcalde", "noun", &alcalde))
        );
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
        let (e, g, _) = primary("de", &de).unwrap();
        assert_eq!(
            (e.pos.as_str(), join_glosses(&g, false).as_str()),
            ("prep", "of; from")
        );

        let mi = full(&[
            r#"{"word":"mi","pos":"noun","senses":[{"glosses":["mi"]}]}"#,
            r#"{"word":"mi","pos":"det","senses":[{"glosses":["apocopic form of mío, my"],"tags":["alt-of"],"alt_of":[{"word":"mío"}]}]}"#,
        ]);
        let (e, g, _) = primary("mi", &mi).unwrap();
        assert_eq!(
            (e.pos.as_str(), join_glosses(&g, false).as_str()),
            ("det", "my")
        );

        let venir = full(&[
            r#"{"word":"venir","pos":"verb","senses":[{"glosses":["Senses relating to literal movement"]},{"glosses":["Senses relating to literal movement","to come"]},{"glosses":["used to express something"]}]}"#,
        ]);
        assert_eq!(
            join_glosses(&primary("venir", &venir).unwrap().1, true),
            "to come"
        );
    }

    #[test]
    fn slang_and_insults_only_when_there_is_nothing_plainer() {
        let perro = full(&[
            r#"{"word":"perro","pos":"noun","senses":[{"glosses":["dog"]},{"glosses":["asshole (despicable person)"],"tags":["derogatory"]},{"glosses":["lazy person"]}]}"#,
        ]);
        assert_eq!(
            join_glosses(&primary("perro", &perro).unwrap().1, false),
            "dog; lazy person"
        );
        // with nothing plainer, the informal sense is still the gloss
        let pasta = full(&[
            r#"{"word":"pasta","pos":"noun","senses":[{"glosses":["dough"],"tags":["slang"]}]}"#,
        ]);
        assert_eq!(
            join_glosses(&primary("pasta", &pasta).unwrap().1, false),
            "dough"
        );
        // colloquial isn't slang: everyday Spain Spanish keeps its place
        let tio = full(&[
            r#"{"word":"tío","pos":"noun","senses":[{"glosses":["uncle"]},{"glosses":["dude, guy"],"tags":["colloquial"]}]}"#,
        ]);
        assert_eq!(
            join_glosses(&primary("tío", &tio).unwrap().1, false),
            "uncle; dude, guy"
        );
    }

    /// (part of speech, gloss) of the entry that defines `word`.
    fn defined(word: &str, lines: &[&str]) -> Option<(String, String)> {
        let entries = full(lines);
        let p = primary(word, &entries)?;
        Some((p.0.pos.clone(), join_glosses(&p.1, p.0.pos == "verb")))
    }

    /// What the word list would hold for `word`, given these dump lines.
    fn detail(word: &str, lines: &[&str]) -> Option<Lemma> {
        curated_detail(word, lines, "")
    }

    /// The same, with `glosses` as the curated glosses file.
    fn curated_detail(word: &str, lines: &[&str], glosses: &str) -> Option<Lemma> {
        let jsonl: Vec<String> = lines.iter().map(|l| l.replace('\n', "")).collect();
        let wanted: HashSet<String> = [word.to_string()].into();
        details(
            jsonl.join("\n").as_bytes(),
            &wanted,
            &index(lines),
            &parse_overrides(glosses).unwrap(),
        )
        .remove(word)
    }

    fn freq(counts: &[(&str, u64)]) -> Vec<(String, u64)> {
        counts.iter().map(|(f, c)| (f.to_string(), *c)).collect()
    }

    #[test]
    fn a_sense_that_names_spain_is_not_regional() {
        let patata = [
            r#"{"word":"patata","pos":"noun","lang_code":"es","senses":[{"glosses":["potato"],"tags":["Philippines","Spain","feminine"]},{"glosses":["piece of rubbish"],"tags":["colloquial","feminine"]}]}"#,
        ];
        assert_eq!(
            defined("patata", &patata).unwrap().1,
            "potato; piece of rubbish"
        );
        // a sense used only outside Spain still gives way to a plainer one
        let carro = [
            r#"{"word":"carro","pos":"noun","lang_code":"es","senses":[{"glosses":["car"],"tags":["Mexico"]},{"glosses":["cart"]}]}"#,
        ];
        assert_eq!(defined("carro", &carro).unwrap().1, "cart");
        // Spain doesn't excuse a sense that is also archaic, and a dialect
        // label alone is still regional
        let idx = index(&[
            r#"{"word":"zumo","pos":"noun","lang_code":"es","senses":[{"glosses":["juice"],"tags":["Equatorial-Guinea","Spain"]}]}"#,
            r#"{"word":"mancebo","pos":"noun","lang_code":"es","senses":[{"glosses":["youth"],"tags":["Spain","archaic"]}]}"#,
            r#"{"word":"guagua","pos":"noun","lang_code":"es","senses":[{"glosses":["bus"],"tags":["Canary-Islands"]}]}"#,
        ]);
        assert!(idx.mainstream("zumo"));
        assert!(!idx.mainstream("mancebo"));
        assert!(!idx.mainstream("guagua"));
        assert_eq!(idx.region("guagua"), Some("latam"));
    }

    #[test]
    fn a_cognate_is_its_own_translation_when_it_is_the_first_meaning() {
        let both = |pos: &str, gloss: &str| Some((pos.to_string(), gloss.to_string()));
        let casino = [
            r#"{"word":"casino","pos":"noun","senses":[{"glosses":["casino"]},{"glosses":["cafeteria, canteen"],"tags":["Chile"]}]}"#,
        ];
        assert_eq!(defined("casino", &casino), both("noun", "casino"));
        let nuclear = [
            r#"{"word":"nuclear","pos":"adj","senses":[{"glosses":["nuclear"]}]}"#,
            r#"{"word":"nuclear","pos":"verb","senses":[{"glosses":["to join up"]}]}"#,
        ];
        assert_eq!(defined("nuclear", &nuclear), both("adj", "nuclear"));
        let bravo = [r#"{"word":"bravo","pos":"intj","senses":[{"glosses":["bravo!"]}]}"#];
        assert_eq!(defined("bravo", &bravo), both("intj", "bravo!"));
        // a later sense that restates the word is a borrowed technical term
        let mesa = [
            r#"{"word":"mesa","pos":"noun","senses":[{"glosses":["table"]},{"glosses":["mesa"]},{"glosses":["desk"]}]}"#,
        ];
        assert_eq!(defined("mesa", &mesa), both("noun", "table; desk"));
        // "Romance" the language family is a name, not the word itself
        let romance = [
            r#"{"word":"romance","pos":"adj","senses":[{"glosses":["Romance"]}]}"#,
            r#"{"word":"romance","pos":"noun","senses":[{"glosses":["romance, love affair"]}]}"#,
        ];
        assert_eq!(
            defined("romance", &romance),
            both("noun", "romance, love affair")
        );
    }

    #[test]
    fn a_word_whose_only_sense_is_a_cognate_reaches_the_list() {
        let idea = detail(
            "idea",
            &[r#"{"word":"idea","pos":"noun","lang_code":"es","senses":[{"glosses":["idea"]}]}"#],
        );
        assert_eq!(idea.map(|l| l.gloss), Some("idea".to_string()));
    }

    #[test]
    fn feminine_and_clipped_nouns_take_the_meaning_they_point_to() {
        let moto = [
            r#"{"word":"moto","pos":"noun","senses":[{"glosses":["clipping of motocicleta; motorcycle"],"tags":["alt-of","clipping","feminine"],"alt_of":[{"word":"motocicleta"}]}]}"#,
            r#"{"word":"moto","pos":"noun","senses":[{"glosses":["milestone"]}]}"#,
        ];
        assert_eq!(defined("moto", &moto).unwrap().1, "motorcycle");
        let maestra = [
            r#"{"word":"maestra","pos":"noun","senses":[{"glosses":["female equivalent of maestro (“teacher”)"],"tags":["feminine","form-of"],"form_of":[{"word":"maestro"}]}]}"#,
            r#"{"word":"maestra","pos":"noun","senses":[{"glosses":["queen bee"]}]}"#,
        ];
        assert_eq!(defined("maestra", &maestra).unwrap().1, "teacher");
        // an entry with a meaning of its own keeps to it
        let prima = [
            r#"{"word":"prima","pos":"noun","senses":[{"glosses":["female equivalent of primo (“cousin”)"],"tags":["feminine","form-of"],"form_of":[{"word":"primo"}]},{"glosses":["bonus"]}]}"#,
        ];
        assert_eq!(defined("prima", &prima).unwrap().1, "bonus");
        // not verbs: a participle's quoted meaning is not a word to teach
        let alabado = [
            r#"{"word":"alabado","pos":"verb","senses":[{"glosses":["past participle of alabar (“praised”)"],"tags":["form-of","participle","past"],"form_of":[{"word":"alabar"}]}]}"#,
        ];
        assert_eq!(defined("alabado", &alabado), None);
        // and not slang
        let jefa = [
            r#"{"word":"jefa","pos":"noun","senses":[{"glosses":["female equivalent of jefe (“old man”)"],"tags":["feminine","form-of","slang"],"form_of":[{"word":"jefe"}]}]}"#,
        ];
        assert_eq!(defined("jefa", &jefa), None);
    }

    const CHILE: &str = r#"{"word":"chile","pos":"noun","lang_code":"es","senses":[{"glosses":["chili pepper"],"tags":["Mexico"]},{"glosses":["penis"],"tags":["Mexico","vulgar"]}]}"#;

    #[test]
    fn vulgar_when_the_meaning_we_would_teach_is_vulgar() {
        // nothing plainer than a regional sense, so the vulgar one is taught too
        let chile = detail("chile", &[CHILE]).unwrap();
        assert_eq!(chile.gloss, "chili pepper; penis");
        assert_eq!(chile.register, Some("vulgar"));
        // one vulgar sense among plain ones is never shown, so it flags nothing
        let madre = detail(
            "madre",
            &[
                r#"{"word":"madre","pos":"noun","lang_code":"es","senses":[{"glosses":["mother"]},{"glosses":["a hell of a lot"],"tags":["Mexico","vulgar"]}]}"#,
            ],
        )
        .unwrap();
        assert_eq!((madre.gloss.as_str(), madre.register), ("mother", None));
        // a curated gloss is what gets taught, whatever Wiktionary leads with
        let curated = curated_detail("chile", &[CHILE], "chile\tchili pepper").unwrap();
        assert_eq!(curated.register, None);
    }

    #[test]
    fn listed_vulgar_words_are_flagged_whatever_their_other_senses() {
        let follar = detail(
            "follar",
            &[
                r#"{"word":"follar","pos":"verb","lang_code":"es","senses":[{"glosses":["to fuck"],"tags":["Spain","vulgar"]},{"glosses":["to compose in sheets"]},{"glosses":["to fart noiselessly"],"tags":["pronominal"]}]}"#,
            ],
        )
        .unwrap();
        assert_eq!(follar.register, Some("vulgar"));
    }

    #[test]
    fn excluded_forms_count_for_nothing_and_never_become_words() {
        // "lindar" is listed for the sake of "lindo", "charles" as a name
        let idx = index(&[
            r#"{"word":"lindo","pos":"adj","lang_code":"es","senses":[{"glosses":["pretty"],"tags":["Latin-America","dated"]}]}"#,
            r#"{"word":"lindo","pos":"verb","lang_code":"es","senses":[{"glosses":["first-person singular present indicative of lindar"],"tags":["form-of"],"form_of":[{"word":"lindar"}]}]}"#,
            r#"{"word":"lindar","pos":"verb","lang_code":"es","senses":[{"glosses":["to abut"]}]}"#,
            r#"{"word":"charles","pos":"verb","lang_code":"es","senses":[{"glosses":["second-person singular present subjunctive of charlar"],"tags":["form-of"],"form_of":[{"word":"charlar"}]}]}"#,
            r#"{"word":"charlar","pos":"verb","lang_code":"es","senses":[{"glosses":["to chat"]}]}"#,
        ]);
        assert_eq!(
            idx.rank(&freq(&[("lindo", 100), ("charles", 50), ("charlar", 3)])),
            [("lindo".to_string(), 100.0), ("charlar".to_string(), 3.0)]
        );
        // an excluded lemma takes no share of a form it has in common
        let idx = index(&[
            r#"{"word":"duele","pos":"verb","lang_code":"es","senses":[{"glosses":["third-person singular present indicative of doler"],"tags":["form-of"],"form_of":[{"word":"doler"}]},{"glosses":["third-person singular present indicative of dolar"],"tags":["form-of"],"form_of":[{"word":"dolar"}]}]}"#,
            r#"{"word":"doler","pos":"verb","lang_code":"es","senses":[{"glosses":["to hurt"]}]}"#,
            r#"{"word":"dolar","pos":"verb","lang_code":"es","senses":[{"glosses":["to hew"]}]}"#,
        ]);
        assert_eq!(
            idx.rank(&freq(&[("duele", 10), ("dolar", 4)])),
            [("doler".to_string(), 10.0)]
        );
    }

    #[test]
    fn excluded_forms_are_no_form_of_any_word() {
        let forms = |l: &Lemma| -> Vec<String> { l.forms.iter().map(|(f, _)| f.clone()).collect() };
        // "china" is the country far more often than the feminine of chino
        let chino = detail(
            "chino",
            &[
                r#"{"word":"chino","pos":"adj","lang_code":"es","senses":[{"glosses":["Chinese"]}],"forms":[{"form":"china","tags":["feminine"]},{"form":"chinos","tags":["masculine","plural"]},{"form":"chinas","tags":["feminine","plural"]}]}"#,
            ],
        )
        .unwrap();
        assert_eq!(forms(&chino), ["chinas", "chino", "chinos"]);
        // "charles" is a name, and a cell of charlar's table as well as a
        // form pass 1 saw; "charlan" is only the latter and stays
        let charlar = detail(
            "charlar",
            &[
                r#"{"word":"charlar","pos":"verb","lang_code":"es","senses":[{"glosses":["to chat"]}],"forms":[
                    {"form":"charlo","tags":["first-person","indicative","present","singular"],"source":"conjugation"},
                    {"form":"charles","tags":["informal","present","second-person","singular","subjunctive"],"source":"conjugation"}]}"#,
                r#"{"word":"charles","pos":"verb","lang_code":"es","senses":[{"glosses":["second-person singular present subjunctive of charlar"],"tags":["form-of"],"form_of":[{"word":"charlar"}]}]}"#,
                r#"{"word":"charlan","pos":"verb","lang_code":"es","senses":[{"glosses":["third-person plural present indicative of charlar"],"tags":["form-of"],"form_of":[{"word":"charlar"}]}]}"#,
            ],
        )
        .unwrap();
        assert_eq!(forms(&charlar), ["charlan", "charlar", "charlo"]);
        // the table itself is whole: "que no charles" is still drilled
        assert!(charlar
            .conjugations
            .iter()
            .any(|c| c.form == "charles" && c.tense == "subjuntivo presente" && c.person == "tú"));
    }

    const IR: &str =
        r#"{"word":"ir","pos":"verb","lang_code":"es","senses":[{"glosses":["to go"]}]}"#;

    #[test]
    fn a_form_listed_as_its_own_word_beats_the_core_verbs() {
        let fuera = [
            r#"{"word":"fuera","pos":"adv","lang_code":"es","senses":[{"glosses":["outside"]},{"glosses":["out"]}]}"#,
            r#"{"word":"fuera","pos":"verb","lang_code":"es","senses":[{"glosses":["first/third-person singular imperfect subjunctive of ser"],"tags":["form-of"],"form_of":[{"word":"ser"}]},{"glosses":["first/third-person singular imperfect subjunctive of ir"],"tags":["form-of"],"form_of":[{"word":"ir"}]}]}"#,
            SER,
            IR,
        ];
        let r = index(&fuera).rank(&freq(&[("fuera", 100), ("ser", 10), ("ir", 10)]));
        assert_eq!(r[0], ("fuera".to_string(), 100.0));
        assert_eq!(r.len(), 3);
        // and its verb senses add nothing to what it means
        let l = detail("fuera", &fuera).unwrap();
        assert_eq!((l.pos.as_str(), l.gloss.as_str()), ("adv", "outside; out"));
    }

    #[test]
    fn words_wiktionary_mislabels_as_latin_american_are_general() {
        let idx = index(&[
            r#"{"word":"herir","pos":"verb","lang_code":"es","senses":[{"glosses":["to hurt, to wound"],"tags":["Latin-America","colloquial","transitive"]},{"glosses":["to offend"],"tags":["Latin-America","colloquial","transitive"]}]}"#,
            r#"{"word":"computadora","pos":"noun","lang_code":"es","senses":[{"glosses":["computer"],"tags":["Latin-America"]}]}"#,
        ]);
        assert_eq!(idx.region("herir"), None);
        assert!(idx.mainstream("herir"));
        assert_eq!(idx.region("computadora"), Some("latam"));
        assert!(!idx.mainstream("computadora"));
    }

    #[test]
    fn only_verbs_carry_conjugation_tables() {
        // "militar" is first of all an adjective; its verb homograph has a table
        let militar = detail(
            "militar",
            &[
                r#"{"word":"militar","pos":"adj","lang_code":"es","senses":[{"glosses":["military"]}],"forms":[{"form":"militares","tags":["plural"]}]}"#,
                &TENER.replace(r#""word":"tener""#, r#""word":"militar""#),
            ],
        )
        .unwrap();
        assert_eq!(militar.pos, "adj");
        assert!(militar.conjugations.is_empty());
        assert!(
            militar
                .forms
                .iter()
                .all(|(_, tag)| !tag.contains('|') && tag != "participio"),
            "{:?}",
            militar.forms
        );
        assert!(militar
            .forms
            .contains(&("militares".into(), "plural".into())));
        // "placer" is first a verb in Wiktionary, but curated as the noun
        let placer = [
            &TENER.replace(r#""word":"tener""#, r#""word":"placer""#),
            r#"{"word":"placer","pos":"noun","lang_code":"es","senses":[{"glosses":["pleasure"]}],"forms":[{"form":"placeres","tags":["plural"]}]}"#,
        ];
        assert!(!detail("placer", &placer).unwrap().conjugations.is_empty());
        let noun = curated_detail("placer", &placer, "placer\tpleasure\tnoun").unwrap();
        assert_eq!(
            (noun.pos.as_str(), noun.gloss.as_str()),
            ("noun", "pleasure")
        );
        assert!(noun.conjugations.is_empty());
        assert!(noun.forms.contains(&("placeres".into(), "plural".into())));
    }

    #[test]
    fn curated_part_of_speech_wins() {
        let veces = [
            r#"{"word":"veces","pos":"prep","lang_code":"es","senses":[{"glosses":["by, times"]}]}"#,
            r#"{"word":"veces","pos":"noun","lang_code":"es","senses":[{"glosses":["plural of vez"],"tags":["form-of","plural"],"form_of":[{"word":"vez"}]}]}"#,
        ];
        let l = curated_detail("veces", &veces, "veces\ttimes (plural of vez)\tnoun").unwrap();
        assert_eq!(
            (l.pos.as_str(), l.gloss.as_str()),
            ("noun", "times (plural of vez)")
        );
        // without the column, the part of speech is still Wiktionary's
        let l = curated_detail("veces", &veces, "veces\ttimes (plural of vez)").unwrap();
        assert_eq!(
            (l.pos.as_str(), l.gloss.as_str()),
            ("prep", "times (plural of vez)")
        );
    }

    #[test]
    fn override_file_errors_name_the_line() {
        let ok = parse_overrides("# comment\n\na\tto\ngato\tcat, tomcat\tnoun\n").unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(
            ok["a"],
            Override {
                gloss: "to".into(),
                pos: None
            }
        );
        assert_eq!(
            ok["gato"],
            Override {
                gloss: "cat, tomcat".into(),
                pos: Some("noun".into())
            }
        );
        for (bad, why) in [
            ("gato\tcat\tnoon", "unknown part of speech"),
            ("gato\tcat\t", "unknown part of speech"),
            ("gato\tcat\tnoun\tx", "expected"),
            ("gato cat", "expected"),
            ("gato\t", "empty lemma or gloss"),
            ("gato\t \tnoun", "empty lemma or gloss"),
            ("\tcat", "empty lemma or gloss"),
            ("gato\tcat\ngato\ttomcat", "lemma listed twice"),
            // an excluded form can never be a word, so a gloss for it is a mistake
            ("dolar\tdollar", "exclude.txt"),
        ] {
            let text = format!("# header\nperro\tdog\n{bad}\n");
            let err = parse_overrides(&text).expect_err(bad);
            let line = 2 + bad.lines().count();
            assert!(
                err.starts_with(&format!("line {line}: ")) && err.contains(why),
                "{bad:?}: {err}"
            );
        }
    }

    #[test]
    fn curation_files_agree() {
        let glosses = parse_overrides(include_str!("../curation/glosses.tsv")).unwrap();
        let lists = [
            ("lemmas.txt", &LEMMAS),
            ("inflections.txt", &INFLECTIONS),
            ("vulgar.txt", &VULGAR),
            ("not_regional.txt", &NOT_REGIONAL),
            ("exclude.txt", &EXCLUDE),
        ];
        for (file, list) in lists {
            assert!(!list.words().is_empty(), "{file} is empty");
            for word in list.words() {
                // lowercase letters only: catches typos and stray spaces
                assert!(is_word(word), "{file}: {word:?} is not a word");
                // an excluded form is never a word, so no other list can want it
                assert!(
                    file == "exclude.txt" || !excluded(word),
                    "{word} is in exclude.txt and {file}"
                );
            }
            // an entry listed twice is a slip of the hand
            let entries = list
                .text
                .lines()
                .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                .count();
            assert_eq!(entries, list.words().len(), "{file} lists a word twice");
        }
        for (word, curated) in &glosses {
            assert!(is_word(word), "glosses.tsv: {word:?} is not a word");
            // the length `join_glosses` keeps to: it has to fit an answer button
            assert!(
                curated.gloss.chars().count() <= 48,
                "glosses.tsv: the gloss of {word} is too long"
            );
        }
        // a word can't be both its own entry and credited to what it inflects
        for word in LEMMAS.words() {
            assert!(!prefer_inflection(word), "{word} is in both lists");
        }
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

    #[test]
    fn joined_glosses_do_not_repeat_themselves() {
        let senses = |s: &[&str]| s.iter().map(|g| g.to_string()).collect::<Vec<_>>();
        assert_eq!(
            join_glosses(&senses(&["and", "plus, and", "well"]), false),
            "and; plus; well"
        );
        assert_eq!(
            join_glosses(&senses(&["father, parent", "father"]), false),
            "father, parent"
        );
        assert_eq!(
            join_glosses(&senses(&["Saturday", "Sabbath, sabbath"]), false),
            "Saturday; Sabbath"
        );
        assert_eq!(
            join_glosses(
                &senses(&["to see, to spot", "to see, to look at, to view"]),
                true
            ),
            "to see, to spot; to look at, to view"
        );
        assert_eq!(
            join_glosses(&senses(&["to hope", "to wait, wait"]), true),
            "to hope; to wait"
        );
        assert_eq!(
            join_glosses(
                &senses(&["to start, begin, to get started", "to begin"]),
                true
            ),
            "to start, begin, to get started"
        );
        // a pronoun's "to you" is not its "you"
        assert_eq!(
            join_glosses(&senses(&["to you, for you", "you", "yourself"]), false),
            "to you, for you; you; yourself"
        );
    }
}
