//! Example sentences: Tatoeba en–es pairs, linked to the words they contain.

use crate::lexicon::excluded;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use tilde_core::srt::strip_accents;

const MAX_SENTENCES: usize = 60_000;
/// Stop picking sentences for a word once it has this many.
const PER_WORD: u32 = 12;
/// Share of a sentence's words that must be in the lexicon, so learners can read it.
const MIN_COVERAGE: f32 = 0.8;

pub struct Pair {
    pub es: String,
    pub en: String,
}

pub fn load_tatoeba(zip_path: &Path) -> Vec<Pair> {
    let file = std::fs::File::open(zip_path).expect("open tatoeba zip");
    let mut zip = zip::ZipArchive::new(file).expect("read tatoeba zip");
    let mut read = |name: &str| {
        let mut s = String::new();
        zip.by_name(name)
            .unwrap_or_else(|e| panic!("{name} in tatoeba zip: {e}"))
            .read_to_string(&mut s)
            .expect("read tatoeba text");
        s
    };
    let es = read("Tatoeba.en-es.es");
    let en = read("Tatoeba.en-es.en");
    let (es, en): (Vec<&str>, Vec<&str>) = (es.lines().collect(), en.lines().collect());
    assert_eq!(es.len(), en.len(), "misaligned Tatoeba pair files");
    es.into_iter()
        .zip(en)
        .map(|(es, en)| Pair {
            es: es.trim().to_string(),
            en: en.trim().to_string(),
        })
        .filter(|p| !p.es.is_empty() && !p.en.is_empty())
        .collect()
}

/// The words of a Spanish sentence, lowercased. Letters that touch a digit
/// are no word: the "CO" of "CO2", the "e" of the chess square "e5" and the
/// "pm" of "2pm" would otherwise be examples of "co", "e" and "pm".
pub fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && t.chars().all(char::is_alphabetic))
        .map(str::to_lowercase)
        .collect()
}

/// The `[a-z']` runs of an English sentence, lowercased: what a gloss is
/// compared with. An apostrophe counts inside a word ("don't", "o'clock"),
/// not around one: 'yes' is "yes", and the travelers' checks are "travelers".
pub fn english_tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace('’', "'")
        .split(|c: char| !c.is_ascii_lowercase() && c != '\'')
        .map(|t| t.trim_matches('\''))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Words of a gloss that say nothing about which word it glosses.
const FUNCTION_WORDS: [&str; 5] = ["a", "an", "the", "of", "to"];
/// Stand-ins in a gloss ("to give something to someone"): no part of the
/// meaning, unless they are all of it ("algo" is "something").
const PLACEHOLDERS: [&str; 7] = [
    "one's",
    "oneself",
    "someone",
    "someone's",
    "something",
    "somebody",
    "somebody's",
];

/// The senses of a gloss, each as the words an English translation must use
/// to show it: "to give birth; to bear (a child)" → [give, birth], [bear].
/// Parentheses hold notes, not meanings. "or" joins two wordings of the same
/// length, and what the longer side has more goes with both: "to rear or
/// raise" is [rear] and [raise], "to be useful or valid" is [be, useful] and
/// [be, valid], "angry or fierce person" is [angry, person] and [fierce,
/// person]. So a word of a phrase never becomes a sense of its own.
pub fn gloss_senses(gloss: &str) -> Vec<Vec<String>> {
    let mut bare = String::new();
    let mut depth = 0u32;
    for c in gloss.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => bare.push(c),
            _ => {}
        }
    }
    let mut senses: Vec<Vec<String>> = Vec::new();
    let mut keep = |words: &[&[Vec<String>]]| {
        let mut sense: Vec<String> = Vec::new();
        for w in words.iter().copied().flatten().flatten() {
            if !sense.contains(w) {
                sense.push(w.clone());
            }
        }
        if !sense.is_empty() && !senses.contains(&sense) {
            senses.push(sense);
        }
    };
    for part in bare.split([';', ',']) {
        // word by word: "medium-sized" is one word of two tokens
        let words: Vec<Vec<String>> = part
            .split_whitespace()
            .map(english_tokens)
            .filter(|w| !w.is_empty())
            .collect();
        let is = |list: &[&str], w: &str| list.contains(&w);
        let means = |w: &str| w.len() >= 2 && !is(&FUNCTION_WORDS, w);
        let only_stand_ins = !words
            .iter()
            .flatten()
            .any(|w| means(w) && !is(&PLACEHOLDERS, w));
        // "or" alone is what "o" means
        let joins = words.len() > 1;
        let mut sides: Vec<Vec<Vec<String>>> = vec![Vec::new()];
        for word in words {
            if joins && word == ["or"] {
                sides.push(Vec::new());
                continue;
            }
            let word: Vec<String> = word
                .into_iter()
                .filter(|w| means(w) && (only_stand_ins || !is(&PLACEHOLDERS, w)))
                .collect();
            if let (false, Some(side)) = (word.is_empty(), sides.last_mut()) {
                side.push(word);
            }
        }
        sides.retain(|side| !side.is_empty());
        match sides.as_slice() {
            // "deliberately or not": the second is no other meaning
            [a, b] if b.concat() == ["not"] => keep(&[a]),
            [a, b] => {
                let n = a.len().min(b.len());
                keep(&[a, &b[n..]]);
                keep(&[&a[..a.len() - n], b]);
            }
            // one wording; or three and more, which no gloss needs told apart
            _ => keep(&sides.iter().map(Vec::as_slice).collect::<Vec<_>>()),
        }
    }
    senses
}

/// English inflections, to tell whether a translation uses a gloss: regular
/// ones by their ending ("dog" in "dogs", see `inflects`), the others from
/// curation/english_forms.tsv ("come" in "came", "big" in "bigger").
pub struct English {
    /// word → its listed forms and the bases it is a listed form of
    related: HashMap<String, Vec<String>>,
    /// The words the list gives forms to: each a word of its own.
    bases: HashSet<String>,
}

impl English {
    /// Reads `base<TAB>form,form,…` lines; `#` comments and blank lines are
    /// skipped. Anything else is an error naming the line.
    pub fn parse(text: &str) -> Result<English, String> {
        let mut related: HashMap<String, Vec<String>> = HashMap::new();
        let mut bases: HashSet<String> = HashSet::new();
        for (n, line) in text.lines().enumerate() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let at = |what: &str| format!("line {}: {what}: {line:?}", n + 1);
            let Some((base, forms)) = line.split_once('\t') else {
                return Err(at("expected base<TAB>form,form"));
            };
            // what `english_tokens` can produce: anything else never matches
            let word =
                |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '\'');
            let forms: Vec<&str> = forms.split(',').map(str::trim).collect();
            if !word(base) || !forms.iter().all(|f| word(f)) {
                return Err(at("not a lowercase English word"));
            }
            if !bases.insert(base.to_string()) {
                return Err(at("base listed twice"));
            }
            for form in forms {
                for (a, b) in [(base, form), (form, base)] {
                    let list = related.entry(a.to_string()).or_default();
                    if a != b && !list.iter().any(|x| x == b) {
                        list.push(b.to_string());
                    }
                }
            }
        }
        Ok(English { related, bases })
    }

    /// Is `token` the gloss word `g`, or a form of it?
    fn is_form_of(&self, g: &str, token: &str) -> bool {
        let is = |t: &str| {
            t == g
                || self
                    .related
                    .get(g)
                    .is_some_and(|r| r.iter().any(|x| x == t))
                // a listed base is no form of a shorter word, whatever it
                // ends in: "evening" is not "even", nor "early" "ear"
                || (!self.bases.contains(t) && inflects(g, t))
        };
        // "the driver's", "the children's": the word before the apostrophe.
        // Not of a negative: "won't" has nothing to do with "won".
        is(token)
            || (!token.ends_with("n't") && token.split_once('\'').is_some_and(|(word, _)| is(word)))
    }

    /// Does the English sentence show one of these senses (see
    /// `gloss_senses`)? Every word of the sense has to be there: "give" alone
    /// doesn't show "to give birth", nor "is" "to be superfluous".
    pub fn supports(&self, senses: &[Vec<String>], en_tokens: &[String]) -> bool {
        senses
            .iter()
            .any(|sense| sense.iter().all(|g| self.has(en_tokens, g)))
    }

    fn has(&self, en_tokens: &[String], g: &str) -> bool {
        en_tokens.iter().any(|t| self.is_form_of(g, t))
    }
}

const VOWELS: &[u8] = b"aeiou";

/// Does the word end in consonant, one vowel, consonant ("stop", "plan",
/// "prefer"; not "need", "help" or "snow")? Such a word doubles its last
/// letter before an ending: "stopped", "planning".
fn doubles(g: &str) -> bool {
    let vowel = |c: u8| VOWELS.contains(&c);
    let [head @ .., before, v, last] = g.as_bytes() else {
        return false;
    };
    // "quit": the u belongs to the q
    let after_consonant = !vowel(*before) || (*before == b'u' && head.ends_with(b"q"));
    after_consonant && vowel(*v) && !vowel(*last) && !b"wxy'".contains(last)
}

/// ...and always does, having that one syllable only: "hatted", never
/// "hated". ("Opened" and "visited" don't, "preferred" does.)
fn must_double(g: &str) -> bool {
    let head = &g[..g.len().saturating_sub(2)];
    let head = head.strip_suffix("qu").unwrap_or(head);
    doubles(g) && !head.bytes().any(|c| VOWELS.contains(&c))
}

/// Is `token` a regular form of the English word `g`: "dogs", "boxes",
/// "walked", "making", "stopped", "tried", "quickly", "you're"? By its
/// ending, not by its beginning ("for" is not in "forget", nor "see" in
/// "seem"), and as English spells it: "hated" is a form of "hate", not of
/// "hat", and "seed" is none of "see". The forms in -er and -est are in the
/// list of irregulars: "bigger" and "swimmer" end like "butter" and "corner".
fn inflects(g: &str, token: &str) -> bool {
    // "he" is not in "her", nor "be" in "best": the forms of words this
    // short are in the list too
    if g.len() < 3 {
        return false;
    }
    let ends = |word: &str, letters: &[u8]| {
        word.bytes()
            .next_back()
            .is_some_and(|c| letters.contains(&c))
    };
    let Some(end) = token.strip_prefix(g) else {
        // the last letter gives way: "make" → "making", "simple" → "simply",
        // "try" → "tried". ("happiness" is another word than "happy".)
        let (stem, last) = g.split_at(g.len() - 1);
        let Some(end) = token.strip_prefix(stem) else {
            return false;
        };
        return match (last, end) {
            // not after a vowel: "see" → "seeing", so "being" is no bee
            ("e", "ing") => !ends(stem, b"eioy"),
            // "possible" → "possibly"; "hole" has nothing to do with "holy"
            ("e", "y") => stem.ends_with('l') && !ends(&stem[..stem.len() - 1], VOWELS),
            // "day" → "days"
            ("y", "ies" | "ied" | "ily") => !ends(stem, VOWELS),
            _ => false,
        };
    };
    match end {
        "s" | "ly" | "n't" => true,
        "es" => ["s", "x", "z", "ch", "sh", "o"]
            .iter()
            .any(|e| g.ends_with(e)),
        // "agreed" is in the list: "seed" and "feed" end that way too
        "d" => g.ends_with('e') && !g.ends_with("ee"),
        "ed" | "ing" => !must_double(g),
        _ if end.starts_with('\'') => true,
        _ => {
            let last = &g[g.len() - 1..];
            doubles(g)
                && end
                    .strip_prefix(last)
                    .is_some_and(|rest| rest == "ed" || rest == "ing")
        }
    }
}

/// A word as the linker needs it.
pub struct LinkWord<'a> {
    pub id: i64,
    pub lemma: &'a str,
    pub gloss: &'a str,
    pub forms: &'a [(String, String)],
    /// Those of `forms` that are only *vos* forms (see `Lemma::voseo`).
    pub voseo: &'a [String],
    /// Never taught: flagged vulgar, or as used only outside Spain (see
    /// `Lemma::register` and `Lemma::region`).
    pub untaught: bool,
}

struct Known {
    /// The lemma, lowercased, and the same without accents.
    headword: String,
    headword_loose: String,
    senses: Vec<Vec<String>>,
}

/// Looks words up by exact form first, falling back to accent-insensitive
/// matching only when there is no exact match (so "de" never links to "dé").
/// A token listed in curation/exclude.txt is no word and finds nothing.
pub struct Linker {
    exact: HashMap<String, Vec<i64>>,
    loose: HashMap<String, Vec<i64>>,
    /// Keys of `exact` and of `loose` that nothing but a *vos* form answers.
    vos_exact: HashSet<String>,
    vos_loose: HashSet<String>,
    words: HashMap<i64, Known>,
    /// The words that are never taught.
    untaught: HashSet<i64>,
    english: English,
}

impl Linker {
    pub fn new<'a>(words: impl IntoIterator<Item = LinkWord<'a>>, english: English) -> Linker {
        let mut exact: HashMap<String, Vec<i64>> = HashMap::new();
        let mut loose: HashMap<String, Vec<i64>> = HashMap::new();
        let mut known: HashMap<i64, Known> = HashMap::new();
        let mut untaught: HashSet<i64> = HashSet::new();
        // keys with a vos reading, and keys with any other reading
        let mut vos: [HashSet<String>; 2] = Default::default();
        let mut plain: [HashSet<String>; 2] = Default::default();
        for w in words {
            if w.untaught {
                untaught.insert(w.id);
            }
            let headword = w.lemma.to_lowercase();
            known.insert(
                w.id,
                Known {
                    headword_loose: strip_accents(&headword),
                    headword,
                    senses: gloss_senses(w.gloss),
                },
            );
            for (form, _) in w.forms {
                let kind = match w.voseo.contains(form) {
                    true => &mut vos,
                    false => &mut plain,
                };
                let form = form.to_lowercase();
                let folded = strip_accents(&form);
                kind[0].insert(form.clone());
                kind[1].insert(folded.clone());
                let e = exact.entry(form).or_default();
                if !e.contains(&w.id) {
                    e.push(w.id);
                }
                let l = loose.entry(folded).or_default();
                if !l.contains(&w.id) {
                    l.push(w.id);
                }
            }
        }
        let [vos_exact, vos_loose] = vos;
        let [plain_exact, plain_loose] = plain;
        Linker {
            exact,
            loose,
            vos_exact: &vos_exact - &plain_exact,
            vos_loose: &vos_loose - &plain_loose,
            words: known,
            untaught,
            english,
        }
    }

    /// Every word `token` could be a form of, and whether it was found as
    /// written (rather than without its accents).
    fn candidates(&self, token: &str) -> (&[i64], bool) {
        // not even without its accent: "Charles" would reach charlar through
        // the voseo form "charlés"
        if excluded(token) {
            return (&[], true);
        }
        match self.exact.get(token) {
            Some(ids) => (ids, true),
            None => (
                self.loose
                    .get(&strip_accents(token))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                false,
            ),
        }
    }

    pub fn lookup(&self, token: &str) -> &[i64] {
        self.candidates(token).0
    }

    /// Whether one of `tokens` can only be a word that is never taught: a
    /// vulgar one, or one that Spain doesn't use. Such a sentence is a poor
    /// example of the other words in it.
    pub fn unsuitable(&self, tokens: &[String]) -> bool {
        tokens.iter().any(|t| {
            let readings = self.lookup(t);
            !readings.is_empty() && readings.iter().all(|id| self.untaught.contains(id))
        })
    }

    /// The first of `tokens` whose only reading is a *vos* form ("tenés",
    /// "sos", "vení"): a sentence with one is Rioplatense, not Castilian.
    /// "Mamá" and "así" are vos forms too (of mamar and asir), but words
    /// first. A token listed in curation/exclude.txt has no reading at all,
    /// so a name spelled like a vos form ("Tomás") goes there.
    pub fn voseo<'t>(&self, tokens: &'t [String]) -> Option<&'t str> {
        tokens.iter().map(String::as_str).find(|t| {
            let (candidates, exact) = self.candidates(t);
            !candidates.is_empty()
                && match exact {
                    true => self.vos_exact.contains(*t),
                    false => self.vos_loose.contains(&strip_accents(t)),
                }
        })
    }

    /// Does the English sentence show this word's gloss: one sense of it, in
    /// full?
    pub fn supports(&self, word_id: i64, en: &[String]) -> bool {
        self.words
            .get(&word_id)
            .is_some_and(|w| self.english.supports(&w.senses, en))
    }

    /// The words `token` is an example of in a sentence translated as `en`
    /// (see `english_tokens`). A token that is a word's own headword belongs
    /// to that word ("padre" is padre, not a form of madre), unless the
    /// English shows another reading and not this one: "vino" with "came" is
    /// venir. A token that is nobody's headword goes to the readings the
    /// English shows ("fue" with "was" is ser), or to all of them when it
    /// shows none. "Shows" is `supports`: a whole sense of the gloss.
    pub fn link(&self, token: &str, en: &[String]) -> Vec<i64> {
        let (candidates, exact) = self.candidates(token);
        if candidates.len() < 2 {
            return candidates.to_vec();
        }
        let keep = |test: &dyn Fn(i64) -> bool| -> Vec<i64> {
            candidates.iter().copied().filter(|id| test(*id)).collect()
        };
        let folded = strip_accents(token);
        let heads = keep(&|id| {
            self.words.get(&id).is_some_and(|w| match exact {
                true => w.headword == token,
                false => w.headword_loose == folded,
            })
        });
        let shown = keep(&|id| self.supports(id, en));
        if heads.is_empty() {
            // a word of a gloss is not the gloss: "are" doesn't make "calles"
            // callar ("to be quiet"), nor "on" "fines" finar ("to pass on")
            return match shown.is_empty() {
                true => candidates.to_vec(),
                false => shown,
            };
        }
        // several heads only without accents: "pápa" for papa and papá
        let shown_heads = keep(&|id| heads.contains(&id) && shown.contains(&id));
        if !shown_heads.is_empty() {
            shown_heads
        } else if shown.is_empty() {
            heads
        } else {
            shown
        }
    }
}

/// A word a sentence is an example of.
#[derive(Debug, PartialEq)]
pub struct Link {
    pub word_id: i64,
    /// The token, lowercased, that is this word: the first one in the sentence.
    pub form: String,
    /// 0 when the English shows the word's gloss, 1 when it doesn't (so the
    /// sentence may use the word in a sense the card doesn't teach).
    pub quality: u8,
}

pub struct Picked {
    pub es: String,
    pub en: String,
    /// One per word, ordered by word id.
    pub links: Vec<Link>,
}

/// Reads `curation/bad_sentences.txt`: one Spanish sentence per line, `#`
/// comments. Compared as `pick` compares them, without case or outer spaces.
pub fn blocklist(text: &str) -> HashSet<String> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(sentence_key)
        .filter(|l| !l.is_empty())
        .collect()
}

fn sentence_key(es: &str) -> String {
    es.trim().to_lowercase()
}

/// The blocklisted sentences that no pair has any more: stale curation.
pub fn not_found<'a>(blocked: &'a HashSet<String>, pairs: &[Pair]) -> Vec<&'a str> {
    let present: HashSet<String> = pairs.iter().map(|p| sentence_key(&p.es)).collect();
    let mut missing: Vec<&str> = blocked
        .iter()
        .filter(|b| !present.contains(*b))
        .map(String::as_str)
        .collect();
    missing.sort_unstable();
    missing
}

/// Short, readable sentences, spread so each word gets a handful of examples.
/// Sentences in `blocked` (see `blocklist`) are never picked, nor are those
/// written with *vos* forms. A sentence with a word that is never taught (a
/// vulgar one, or one used only outside Spain) comes after all the others:
/// it is picked only for words still short of examples, and the app, which
/// shows a word's examples in this order, shows it last.
pub fn pick(pairs: &[Pair], linker: &Linker, blocked: &HashSet<String>) -> Vec<Picked> {
    struct Candidate<'a> {
        pair: &'a Pair,
        links: Vec<Link>,
        unsuitable: bool,
        coverage: f32,
        len: usize,
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for pair in pairs {
        let toks = tokens(&pair.es);
        let key = sentence_key(&pair.es);
        if !(3..=18).contains(&toks.len())
            || blocked.contains(&key)
            || linker.voseo(&toks).is_some()
            || !seen.insert(key)
        {
            continue;
        }
        let en = english_tokens(&pair.en);
        let mut links: Vec<Link> = Vec::new();
        let mut linked = 0usize;
        for t in &toks {
            // readability is about knowing the word at all, whichever
            // reading the example ends up under
            if linker.lookup(t).is_empty() {
                continue;
            }
            linked += 1;
            for word_id in linker.link(t, &en) {
                if !links.iter().any(|l| l.word_id == word_id) {
                    links.push(Link {
                        word_id,
                        form: t.clone(),
                        quality: u8::from(!linker.supports(word_id, &en)),
                    });
                }
            }
        }
        links.sort_by_key(|l| l.word_id);
        let coverage = linked as f32 / toks.len() as f32;
        if coverage >= MIN_COVERAGE {
            candidates.push(Candidate {
                pair,
                links,
                unsuitable: linker.unsuitable(&toks),
                coverage,
                len: toks.len(),
            });
        }
    }
    candidates.sort_by(|a, b| {
        a.unsuitable
            .cmp(&b.unsuitable)
            .then(b.coverage.total_cmp(&a.coverage))
            .then(a.len.cmp(&b.len))
    });

    let mut per_word: HashMap<i64, u32> = HashMap::new();
    let mut out = Vec::new();
    for c in candidates {
        if out.len() >= MAX_SENTENCES {
            break;
        }
        if !c
            .links
            .iter()
            .any(|l| per_word.get(&l.word_id).copied().unwrap_or(0) < PER_WORD)
        {
            continue;
        }
        for l in &c.links {
            *per_word.entry(l.word_id).or_default() += 1;
        }
        out.push(Picked {
            es: c.pair.es.clone(),
            en: c.pair.en.clone(),
            links: c.links,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Word {
        id: i64,
        lemma: &'static str,
        gloss: &'static str,
        forms: Vec<(String, String)>,
        voseo: Vec<String>,
        untaught: bool,
    }

    /// A word with its own row and the given (form, tag) rows.
    fn word(id: i64, lemma: &'static str, gloss: &'static str, forms: &[(&str, &str)]) -> Word {
        let mut all = vec![(lemma.to_string(), String::new())];
        all.extend(forms.iter().map(|(f, t)| (f.to_string(), t.to_string())));
        Word {
            id,
            lemma,
            gloss,
            forms: all,
            voseo: Vec::new(),
            untaught: false,
        }
    }

    /// A verb whose `vos` forms are only that.
    fn verb(
        id: i64,
        lemma: &'static str,
        gloss: &'static str,
        forms: &[(&str, &str)],
        vos: &[&str],
    ) -> Word {
        let mut verb = word(id, lemma, gloss, forms);
        for v in vos {
            verb.forms.push((v.to_string(), String::new()));
            verb.voseo.push(v.to_string());
        }
        verb
    }

    fn english() -> English {
        English::parse(include_str!("../curation/english_forms.tsv")).unwrap()
    }

    fn linker(words: &[Word]) -> Linker {
        Linker::new(
            words.iter().map(|w| LinkWord {
                id: w.id,
                lemma: w.lemma,
                gloss: w.gloss,
                forms: &w.forms,
                voseo: &w.voseo,
                untaught: w.untaught,
            }),
            english(),
        )
    }

    fn pair(es: &str, en: &str) -> Pair {
        Pair {
            es: es.into(),
            en: en.into(),
        }
    }

    fn link(word_id: i64, form: &str, quality: u8) -> Link {
        Link {
            word_id,
            form: form.into(),
            quality,
        }
    }

    const MADRE: i64 = 165;
    const PADRE: i64 = 122;

    /// madre as the forms table had it: with its masculine counterpart.
    fn family() -> Vec<Word> {
        vec![
            word(
                MADRE,
                "madre",
                "mother",
                &[
                    ("madres", "plural"),
                    ("padre", "masculine"),
                    ("padres", "masculine plural"),
                ],
            ),
            word(PADRE, "padre", "father, parent", &[("padres", "plural")]),
            word(21, "mi", "my", &[("mis", "plural")]),
            word(4, "y", "and", &[]),
        ]
    }

    #[test]
    fn exact_forms_win_over_accent_insensitive_ones() {
        let linker = linker(&[
            word(1, "de", "of; from", &[]),
            word(2, "dar", "to give", &[("dé", "subjuntivo presente|yo")]),
            word(3, "estar", "to be", &[("está", "presente|él/ella/usted")]),
        ]);
        assert_eq!(linker.lookup("de"), [1]);
        assert_eq!(linker.lookup("dé"), [2]);
        assert_eq!(linker.lookup("esta"), [3]);
        assert!(linker.lookup("nada").is_empty());
        assert!(linker.link("nada", &[]).is_empty());
    }

    #[test]
    fn letters_that_touch_a_digit_are_no_word() {
        assert_eq!(
            tokens("El CO2 y el H₂O, a las 2pm (casilla e5)."),
            ["el", "y", "el", "a", "las", "casilla"]
        );
        // a number standing alone takes no word with it
        assert_eq!(tokens("Tengo 30 años."), ["tengo", "años"]);
    }

    #[test]
    fn a_token_listed_as_a_non_word_links_to_nothing() {
        // "charles" and "linea" are in curation/exclude.txt: a name that the
        // voseo form "charlés" would answer for, and a misspelling
        let linker = linker(&[
            word(1, "charlar", "to chat", &[("charlés", "")]),
            word(2, "línea", "line", &[]),
        ]);
        assert!(linker.lookup("charles").is_empty());
        assert!(linker.lookup("linea").is_empty());
        assert!(linker
            .link("charles", &english_tokens("Charles likes to chat."))
            .is_empty());
        assert_eq!(linker.lookup("charlés"), [1]);
        assert_eq!(linker.lookup("línea"), [2]);
    }

    #[test]
    fn tokens_keep_accents_and_enye() {
        assert_eq!(
            tokens("¿Dónde está el niño?"),
            ["dónde", "está", "el", "niño"]
        );
    }

    #[test]
    fn english_tokens_keep_an_apostrophe_inside_a_word_only() {
        assert_eq!(
            english_tokens("Don't say 'yes' at 5 o'clock."),
            ["don't", "say", "yes", "at", "o'clock"]
        );
        // the plural's possessive, a dropped letter, a typographic apostrophe
        assert_eq!(
            english_tokens("The travelers' checks; rock 'n' roll; it’s ‘his’."),
            [
                "the",
                "travelers",
                "checks",
                "rock",
                "n",
                "roll",
                "it's",
                "his"
            ]
        );
        let english = english();
        let supports =
            |gloss: &str, en: &str| english.supports(&gloss_senses(gloss), &english_tokens(en));
        assert!(supports("yes", "He said 'yes'."));
        assert!(supports("traveler", "Do you take travelers' checks?"));
        assert!(supports("to be", "It’s late."));
    }

    #[test]
    fn gloss_senses_leave_out_notes_and_function_words() {
        let senses = |gloss: &str| -> Vec<String> {
            gloss_senses(gloss).iter().map(|s| s.join(" ")).collect()
        };
        assert_eq!(senses("to come"), ["come"]);
        assert_eq!(senses("new (feminine); news"), ["new", "news"]);
        assert_eq!(senses("the"), [] as [&str; 0]);
        assert_eq!(
            senses("to put on (clothes); to dress oneself"),
            ["put on", "dress"]
        );
        // a note with a comma in it is still one note
        assert_eq!(senses("lady; queen (chess, cards)"), ["lady", "queen"]);
        assert_eq!(senses("one's own; a, I"), ["own"]);
        assert_eq!(senses("o'clock; Father, father"), ["o'clock", "father"]);
        // two wordings of one sense
        assert_eq!(
            senses("to give birth; to rear or raise"),
            ["give birth", "rear", "raise"]
        );
        assert_eq!(
            senses("finger or toe; thimble"),
            ["finger", "toe", "thimble"]
        );
        assert_eq!(senses("to go up or come up"), ["go up", "come up"]);
        // two single words are two wordings, be the phrase an idiom or not
        assert_eq!(senses("more or less"), ["more", "less"]);
        assert_eq!(senses("or; either"), ["or", "either"]);
        assert_eq!(
            senses("an examination, test, or quiz"),
            ["examination", "test", "quiz"]
        );
        // what the longer wording has more goes with both: no word of a
        // phrase is a sense of its own ("large" is not what serpiente means)
        assert_eq!(
            senses("snake; a large or medium-sized snake"),
            ["snake", "large snake", "medium sized snake"]
        );
        assert_eq!(
            senses("tiger; angry or fierce person"),
            ["tiger", "angry person", "fierce person"]
        );
        assert_eq!(
            senses("wall, especially of a house or room"),
            ["wall", "especially house", "especially room"]
        );
        assert_eq!(
            senses("to be worth; to be useful or valid"),
            ["be worth", "be useful", "be valid"]
        );
        assert_eq!(
            senses("to regret, feel or be sorry"),
            ["regret", "feel sorry", "be sorry"]
        );
        assert_eq!(
            senses("to touch; to be someone's time or turn"),
            ["touch", "be time", "be turn"]
        );
        assert_eq!(
            senses("of, from or relating to Misiones"),
            ["from misiones", "relating misiones"]
        );
        // "or not" is no second meaning
        assert_eq!(
            senses("to find something, deliberately or not"),
            ["find", "deliberately"]
        );
        // three wordings: none is told apart
        assert_eq!(
            senses("to raise or hoist a standard or flag"),
            ["raise hoist standard flag"]
        );
        // a stand-in is the meaning where the gloss is nothing else
        assert_eq!(
            senses("something, anything; to give something to someone"),
            ["something", "anything", "give"]
        );
    }

    #[test]
    fn english_support_matches_inflections_and_irregulars() {
        let english = english();
        let supports =
            |gloss: &str, en: &str| english.supports(&gloss_senses(gloss), &english_tokens(en));
        // irregular forms, both ways
        assert!(supports("to come", "He came running."));
        assert!(supports("to send", "I sent it"));
        assert!(supports("to be", "It was easy."));
        assert!(supports("better", "That's a good one."));
        // regular ones by their ending
        assert!(supports("dog", "two dogs"));
        assert!(supports("to make", "She's making tea."));
        assert!(supports("to carry", "He carried it."));
        assert!(supports("to help", "Tom helped me."));
        assert!(supports("to stop", "He stopped running."));
        assert!(supports("to run", "He stopped running."));
        assert!(supports("big; easy", "It's bigger."));
        assert!(supports("big; easy", "Easily done."));
        assert!(supports("possible", "Possibly."));
        assert!(supports("you", "You're late."));
        assert!(supports("box", "Two boxes."));
        assert!(supports("to drive; child", "The driver's seat."));
        assert!(supports("to drive; child", "The children's room."));
        assert!(supports("can; not", "I couldn't."));
        assert!(supports("can", "You can't."));
        assert!(supports("luck", "You're lucky."));
        assert!(supports("to be afraid", "I'm afraid not."));
        assert!(!supports("to win; to do", "I won't. I don't."));
        // but not a longer word that merely starts the same, or a short one
        assert!(!supports("happy", "happiness"));
        assert!(!supports("to go", "It's good."));
        assert!(!supports("to be", "The best one."));
        assert!(!supports("for", "Don't forget the fork."));
        assert!(!supports("to see", "It seems so."));
        assert!(!supports("many", "Look at the mangoes."));
        assert!(!supports("wine; note", "Not at the window."));
        assert!(!supports("you; here", "Your hat and her hat."));
        assert!(!supports("car; hat", "Who cares? He hates it."));
        assert!(!supports("bee", "I have been there."));
        assert!(!supports("bee", "A human being."));
        // nor a word that only ends like a form. A short word doubles its
        // last letter ("hatted"), so "hated" is not its form:
        assert!(!supports("car; hat", "He hated it and cared."));
        assert!(!supports("not", "He noted it."));
        assert!(!supports("star", "He stared at the scar."));
        assert!(!supports("scar", "I'm scared."));
        assert!(supports("to hate; to care", "He hated it."));
        assert!(supports("to plan", "We planned it."));
        assert!(supports("to open; to visit", "He opened it."));
        assert!(supports("to open; to visit", "She visited us."));
        assert!(supports("to prefer", "I preferred it."));
        assert!(supports("to quit", "He's quitting."));
        // the verbs in -ee have their past in the list:
        assert!(!supports("to see", "A seed."));
        assert!(!supports("fee", "Feed the dog."));
        assert!(supports("to agree; to free", "We agreed."));
        assert!(supports("to agree; to free", "They were freed."));
        assert!(supports("to see", "Seeing is believing."));
        // and so have the words that take -er and -est:
        assert!(!supports("but", "Pass the butter."));
        assert!(!supports("let", "A letter came."));
        assert!(!supports("for", "In the forest."));
        assert!(!supports("to bet", "It's better."));
        assert!(!supports("sum", "In summer."));
        assert!(!supports("corn", "At the corner."));
        assert!(supports("hot; nice", "It's hotter."));
        assert!(supports("hot; nice", "The nicest one."));
        assert!(supports("happy", "I'm happier now."));
        assert!(supports("to swim", "He's a good swimmer."));
        // a word the list has as a base is a word of its own
        assert!(!supports("even", "Good evening."));
        assert!(!supports("ear", "Come early."));
        assert!(supports("even", "It evened out."));
        assert!(supports("early", "Come earlier."));
        assert!(!supports("hole", "Holy cow!"));
        assert!(supports("day", "My daily bread."));
        assert!(!supports("day", "Daisies."));
        // a form's other forms are not its relatives: "was" is not "is"
        assert!(!supports("was", "It is."));
        // a sense of several words needs them all
        assert!(supports("to give birth", "She gave birth to twins."));
        assert!(!supports("to give birth", "Give me time."));
        assert!(!supports("to be superfluous", "This is a paper."));
        assert!(supports("to be worth", "It's worth a fortune."));
        assert!(supports("to fall in love; to charm", "He fell in love."));
        assert!(supports(
            "to fall in love; to charm",
            "She charms everyone."
        ));
        assert!(supports("to put on", "Put your coat on."));
        // nothing to compare
        assert!(!supports("the", "the end"));
        assert!(!supports("new (feminine); news", "a feminine form"));
        assert!(!supports("to come", ""));
    }

    #[test]
    fn english_forms_file_errors_name_the_line() {
        let ok = English::parse("# irregular\n\ncome\tcame, coming\ngood\tbetter,best\n").unwrap();
        assert!(ok.is_form_of("come", "came"));
        assert!(ok.is_form_of("came", "come"));
        assert!(!ok.is_form_of("came", "coming"));
        assert!(ok.is_form_of("come", "coming"));
        for (bad, why) in [
            ("come came", "expected"),
            ("come\t", "not a lowercase English word"),
            ("\tcame", "not a lowercase English word"),
            ("must\thad to", "not a lowercase English word"),
            ("Come\tcame", "not a lowercase English word"),
            ("go\twent\ngo\tgone", "base listed twice"),
        ] {
            let text = format!("# header\nbe\twas\n{bad}\n");
            let err = English::parse(&text).err().expect(bad);
            let line = 2 + bad.lines().count();
            assert!(
                err.starts_with(&format!("line {line}: ")) && err.contains(why),
                "{bad:?}: {err}"
            );
        }
    }

    #[test]
    fn a_token_that_is_a_lemma_links_to_that_lemma() {
        let linker = linker(&family());
        let en = english_tokens("My father grew old.");
        assert_eq!(linker.lookup("padre"), [MADRE, PADRE]);
        assert_eq!(linker.link("padre", &en), [PADRE]);
        // and with nothing in the English to go by
        assert_eq!(linker.link("padre", &[]), [PADRE]);
        // a headword written without its accent is still that headword
        let linker = linker_of_tia();
        assert_eq!(linker.link("tia", &english_tokens("My aunt.")), [822]);
    }

    fn linker_of_tia() -> Linker {
        linker(&[
            word(
                284,
                "tío",
                "uncle; guy",
                &[("tía", "feminine"), ("tíos", "plural")],
            ),
            word(822, "tía", "aunt; girl", &[("tías", "plural")]),
        ])
    }

    #[test]
    fn english_moves_a_homograph_to_the_word_it_translates() {
        let linker = linker(&[
            word(700, "vino", "wine", &[("vinos", "plural")]),
            word(
                60,
                "venir",
                "to come",
                &[("vino", "pretérito|él/ella/usted")],
            ),
        ]);
        let came = english_tokens("He came running.");
        let wine = english_tokens("Do you drink wine?");
        assert_eq!(linker.link("vino", &came), [60]);
        assert_eq!(linker.link("vino", &wine), [700]);
        assert_eq!(linker.lookup("vino"), [700, 60]);
    }

    #[test]
    fn the_headword_wins_when_the_english_supports_both_or_neither() {
        let linker = linker(&[
            word(300, "ayuda", "help", &[("ayudas", "plural")]),
            word(
                150,
                "ayudar",
                "to help",
                &[("ayuda", "presente|él/ella/usted")],
            ),
            word(30, "como", "how; as; like", &[]),
            word(200, "comer", "to eat", &[("como", "presente|yo")]),
        ]);
        // both: "help" is the noun's gloss and the verb's
        let en = english_tokens("I need your help.");
        assert_eq!(linker.link("ayuda", &en), [300]);
        // neither
        let en = english_tokens("Bread, please.");
        assert_eq!(linker.link("como", &en), [30]);
        // only the other reading: the English decides
        let en = english_tokens("I eat bread.");
        assert_eq!(linker.link("como", &en), [200]);
    }

    #[test]
    fn a_non_lemma_token_follows_the_english_or_keeps_all() {
        let linker = linker(&[
            word(3, "ser", "to be", &[("fue", "pretérito|él/ella/usted")]),
            word(15, "ir", "to go", &[("fue", "pretérito|él/ella/usted")]),
        ]);
        assert_eq!(linker.link("fue", &english_tokens("It was easy.")), [3]);
        assert_eq!(linker.link("fue", &english_tokens("He went home.")), [15]);
        assert_eq!(linker.link("fue", &english_tokens("Tom did it.")), [3, 15]);
    }

    #[test]
    fn a_headword_gives_way_only_to_a_whole_sense() {
        let linker = linker(&[
            word(27, "para", "for, to; in order to, so", &[]),
            word(
                929,
                "parir",
                "to give birth",
                &[("para", "subjuntivo presente|yo")],
            ),
            word(
                466,
                "parar",
                "to stop, to halt",
                &[("para", "presente|él/ella/usted")],
            ),
        ]);
        // "give" is not "to give birth"
        let en = english_tokens("Give me time to think it over.");
        assert_eq!(linker.link("para", &en), [27]);
        let en = english_tokens("She gave birth to twins, so they say.");
        assert_eq!(linker.link("para", &en), [27]);
        let en = english_tokens("May she give birth in peace.");
        assert_eq!(linker.link("para", &en), [929]);
        let en = english_tokens("Stop criticizing me!");
        assert_eq!(linker.link("para", &en), [466]);
    }

    #[test]
    fn a_word_of_a_gloss_shows_no_reading() {
        let linker = linker(&[
            word(494, "calle", "street; lane", &[("calles", "plural")]),
            word(
                767,
                "callar",
                "to be quiet, to keep silent",
                &[("calles", "subjuntivo presente|tú")],
            ),
            word(339, "fin", "end; purpose, aim", &[("fines", "plural")]),
            word(
                9067,
                "finar",
                "to pass on, pass away",
                &[("fines", "subjuntivo presente|tú")],
            ),
            word(
                35,
                "ver",
                "to see, to spot; to look at",
                &[("ves", "presente|tú")],
            ),
            word(15, "ir", "to go", &[("ves", "imperativo|tú")]),
        ]);
        // "are" is in "to be quiet" and "on" in "to pass on": that says
        // nothing, and the street and the weekend stay what they are
        let en = english_tokens("The roads are dirty.");
        assert_eq!(linker.link("calles", &en), [494, 767]);
        let en = english_tokens("I work on weekends.");
        assert_eq!(linker.link("fines", &en), [339, 9067]);
        // nor does "look" without its "at"
        let en = english_tokens("You look pale.");
        assert_eq!(linker.link("ves", &en), [35, 15]);
        // a whole sense does
        let en = english_tokens("The streets are dirty.");
        assert_eq!(linker.link("calles", &en), [494]);
        let en = english_tokens("Don't be quiet about it.");
        assert_eq!(linker.link("calles", &en), [767]);
        assert_eq!(linker.link("ves", &english_tokens("Go and look!")), [15]);
        assert_eq!(linker.link("ves", &english_tokens("Look at me.")), [35]);
    }

    #[test]
    fn headwords_that_differ_only_in_accents_follow_the_english() {
        // a slip of the accent: neither word is written "pápa"
        let linker = linker(&[
            word(900, "papa", "potato; pope", &[]),
            word(400, "papá", "dad", &[]),
            word(
                500,
                "papar",
                "to gulp down",
                &[("papa", "presente|él/ella/usted")],
            ),
        ]);
        assert_eq!(
            linker.link("pápa", &english_tokens("My dad is here.")),
            [400]
        );
        assert_eq!(linker.link("pápa", &english_tokens("One potato.")), [900]);
        assert_eq!(linker.link("pápa", &english_tokens("Hello!")), [900, 400]);
        // and both give way together when only a third reading is shown
        assert_eq!(
            linker.link("pápa", &english_tokens("He gulps it down.")),
            [500]
        );
    }

    #[test]
    fn picks_readable_sentences_only() {
        let linker = linker(&[
            word(1, "yo", "I", &[]),
            word(2, "tener", "to have", &[("tengo", "presente|yo")]),
            word(3, "un", "a, an", &[]),
            word(4, "perro", "dog", &[]),
        ]);
        let pairs = [
            pair("Yo tengo un perro.", "I have a dog."),
            pair("Yo tengo un perro.", "Duplicate."),
            pair("Yo tengo un xilófono azulado.", "Too many unknown words."),
            pair("Perro.", "Too short."),
        ];
        let picked = pick(&pairs, &linker, &HashSet::new());
        assert_eq!(picked.len(), 1);
        let ids: Vec<i64> = picked[0].links.iter().map(|l| l.word_id).collect();
        assert_eq!(ids, [1, 2, 3, 4]);
    }

    #[test]
    fn links_record_their_token_and_quality() {
        let mut words = family();
        words.push(word(700, "vino", "wine", &[]));
        words.push(word(
            60,
            "venir",
            "to come",
            &[("vino", "pretérito|él/ella/usted")],
        ));
        let linker = linker(&words);
        let pairs = [
            pair("Mi madre y mi padre.", "My mother and my father."),
            // nothing in the English says which "vino" this is
            pair("Mis padres y el vino.", "Dinner with the family."),
        ];
        let picked = pick(&pairs, &linker, &HashSet::new());
        assert_eq!(picked.len(), 2);
        assert_eq!(
            picked[0].links,
            [
                link(4, "y", 0),
                link(21, "mi", 0),
                link(PADRE, "padre", 0),
                link(MADRE, "madre", 0),
            ]
        );
        // "padres" is nobody's headword and the English shows neither parent:
        // both keep it, unconfirmed. "vino" is the noun's headword.
        assert_eq!(
            picked[1].links,
            [
                link(4, "y", 1),
                link(21, "mis", 1),
                link(PADRE, "padres", 1),
                link(MADRE, "padres", 1),
                link(700, "vino", 1),
            ]
        );
    }

    #[test]
    fn a_word_is_linked_by_its_first_token() {
        let linker = linker(&[
            word(
                2,
                "tener",
                "to have",
                &[("tengo", "presente|yo"), ("tienes", "presente|tú")],
            ),
            word(5, "lo", "it", &[]),
            word(6, "que", "that; than", &[]),
        ]);
        let picked = pick(
            &[pair("Tengo lo que tienes.", "I have what you have.")],
            &linker,
            &HashSet::new(),
        );
        assert_eq!(
            picked[0].links,
            [link(2, "tengo", 0), link(5, "lo", 1), link(6, "que", 1)]
        );
    }

    #[test]
    fn sentences_written_with_vos_forms_are_not_picked() {
        let linker = linker(&[
            verb(
                15,
                "tener",
                "to have",
                &[("tienes", "presente|tú"), ("ten", "imperativo|tú")],
                &["tenés", "tené"],
            ),
            verb(900, "mamar", "to suckle", &[], &["mamá", "mamás"]),
            verb(
                950,
                "finar",
                "to die",
                &[("fines", "subjuntivo presente|tú")],
                &["finés"],
            ),
            verb(960, "charlar", "to chat", &[], &["charlés"]),
            word(400, "mamá", "mom", &[("mamás", "plural")]),
            word(500, "hambre", "hunger", &[]),
            word(21, "mi", "my", &[]),
            word(7, "fin", "end", &[("fines", "plural")]),
        ]);
        let voseo = |es: &str| linker.voseo(&tokens(es)).map(str::to_string);
        assert_eq!(voseo("¿Tenés hambre?"), Some("tenés".into()));
        assert_eq!(voseo("¿Tienes hambre?"), None);
        // written without its accent it is still nothing else
        assert_eq!(voseo("Tene cuidado, tenes hambre."), Some("tene".into()));
        // a vos form that is a word of its own, or another word's form, is that
        assert_eq!(voseo("Mi mamá tiene dos mamás."), None);
        // "fines" is found as written, so the vos form "finés" isn't asked
        assert_eq!(voseo("Con fines de lucro."), None);
        assert_eq!(voseo("No finés."), Some("finés".into()));
        // a name in curation/exclude.txt has no reading, so it is no vos form
        // either: "Charles" is not "vos charlés"
        assert_eq!(voseo("No charlés tanto."), Some("charlés".into()));
        assert_eq!(voseo("Charles tiene hambre."), None);

        let pairs = [
            pair("¿Tenés hambre, mamá?", "Are you hungry, mom?"),
            pair("¿Tienes hambre, mamá?", "Are you hungry, mom?"),
        ];
        let picked = pick(&pairs, &linker, &HashSet::new());
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].es, "¿Tienes hambre, mamá?");
        // "mamá" is mamar's vos form as well: a token that is a headword
        // belongs to that word
        let ids: Vec<i64> = picked[0].links.iter().map(|l| l.word_id).collect();
        assert_eq!(ids, [15, 400, 500]);
    }

    #[test]
    fn blocklisted_sentences_are_not_picked() {
        let linker = linker(&[
            word(8, "el", "the", &[]),
            word(
                300,
                "depender",
                "to depend",
                &[("depende", "presente|él/ella/usted")],
            ),
            word(900, "contexto", "context", &[]),
            word(7, "de", "of; from", &[("del", "")]),
        ]);
        let blocked = blocklist("# ungrammatical\n\n  Depende el contexto.\n");
        assert_eq!(blocked.len(), 1);
        let pairs = [
            pair("Depende el contexto.", "It depends on the context."),
            pair("DEPENDE el contexto. ", "It depends on the context."),
            pair("Depende del contexto.", "It depends on the context."),
        ];
        let picked = pick(&pairs, &linker, &blocked);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].es, "Depende del contexto.");
        // without the list both spellings are one sentence, picked once
        assert_eq!(pick(&pairs, &linker, &HashSet::new()).len(), 2);
        assert!(not_found(&blocked, &pairs).is_empty());
        assert_eq!(not_found(&blocked, &pairs[2..]), ["depende el contexto."]);
    }

    #[test]
    fn a_sentence_with_a_word_that_is_never_taught_comes_last() {
        let mut culo = word(30, "culo", "arse", &[]);
        culo.untaught = true;
        // "pavo" is a turkey, and a flagged word too: a reading that isn't
        // flagged is enough for a token to be an ordinary one
        let mut pavo = word(31, "pavo", "idiot", &[]);
        pavo.untaught = true;
        // Spain says "ordenador"
        let mut computadora = word(32, "computadora", "computer", &[]);
        computadora.untaught = true;
        let linker = linker(&[
            word(
                1,
                "besar",
                "to kiss",
                &[("bésame", ""), ("besa", "presente|él/ella/usted")],
            ),
            word(2, "mi", "my", &[]),
            word(3, "mano", "hand", &[]),
            word(4, "el", "the", &[("la", "feminine")]),
            word(5, "pavo", "turkey", &[]),
            culo,
            pavo,
            computadora,
        ]);
        let pairs = [
            pair("Bésame el culo.", "Kiss my arse."),
            pair("Besa la computadora.", "He kisses the computer."),
            pair("Besa el pavo.", "He kisses the turkey."),
            pair("Bésame la mano.", "Kiss my hand."),
        ];
        let picked = pick(&pairs, &linker, &HashSet::new());
        let picked: Vec<&str> = picked.iter().map(|p| p.es.as_str()).collect();
        // the first two would otherwise lead: they are as short as the
        // others, all their words are known, and they come first in the file
        assert_eq!(
            picked,
            [
                "Besa el pavo.",
                "Bésame la mano.",
                "Bésame el culo.",
                "Besa la computadora."
            ]
        );
        // each is still an example, of the flagged word too
        assert!(linker.unsuitable(&tokens("Bésame el culo.")));
        assert!(!linker.unsuitable(&tokens("Besa el pavo.")));
    }
}
