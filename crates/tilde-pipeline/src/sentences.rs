//! Example sentences: Tatoeba en–es pairs, linked to the words they contain.

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

pub fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphabetic())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Looks words up by exact form first, falling back to accent-insensitive
/// matching only when there is no exact match (so "de" never links to "dé").
pub struct Linker {
    exact: HashMap<String, Vec<i64>>,
    loose: HashMap<String, Vec<i64>>,
}

impl Linker {
    pub fn new<'a>(forms: impl IntoIterator<Item = (&'a str, i64)>) -> Linker {
        let mut exact: HashMap<String, Vec<i64>> = HashMap::new();
        let mut loose: HashMap<String, Vec<i64>> = HashMap::new();
        for (form, id) in forms {
            let form = form.to_lowercase();
            let e = exact.entry(form.clone()).or_default();
            if !e.contains(&id) {
                e.push(id);
            }
            let l = loose.entry(strip_accents(&form)).or_default();
            if !l.contains(&id) {
                l.push(id);
            }
        }
        Linker { exact, loose }
    }

    pub fn lookup(&self, token: &str) -> &[i64] {
        self.exact
            .get(token)
            .or_else(|| self.loose.get(&strip_accents(token)))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

pub struct Picked {
    pub es: String,
    pub en: String,
    pub word_ids: Vec<i64>,
}

/// Short, readable sentences, spread so each word gets a handful of examples.
pub fn pick(pairs: &[Pair], linker: &Linker) -> Vec<Picked> {
    struct Candidate<'a> {
        pair: &'a Pair,
        ids: Vec<i64>,
        coverage: f32,
        len: usize,
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for pair in pairs {
        let toks = tokens(&pair.es);
        if !(3..=18).contains(&toks.len()) || !seen.insert(pair.es.to_lowercase()) {
            continue;
        }
        let mut ids: Vec<i64> = Vec::new();
        let mut linked = 0usize;
        for t in &toks {
            let hits = linker.lookup(t);
            if !hits.is_empty() {
                linked += 1;
                ids.extend_from_slice(hits);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        let coverage = linked as f32 / toks.len() as f32;
        if coverage >= MIN_COVERAGE {
            candidates.push(Candidate {
                pair,
                ids,
                coverage,
                len: toks.len(),
            });
        }
    }
    candidates.sort_by(|a, b| b.coverage.total_cmp(&a.coverage).then(a.len.cmp(&b.len)));

    let mut per_word: HashMap<i64, u32> = HashMap::new();
    let mut out = Vec::new();
    for c in candidates {
        if out.len() >= MAX_SENTENCES {
            break;
        }
        if !c
            .ids
            .iter()
            .any(|id| per_word.get(id).copied().unwrap_or(0) < PER_WORD)
        {
            continue;
        }
        for id in &c.ids {
            *per_word.entry(*id).or_default() += 1;
        }
        out.push(Picked {
            es: c.pair.es.clone(),
            en: c.pair.en.clone(),
            word_ids: c.ids,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_forms_win_over_accent_insensitive_ones() {
        let linker = Linker::new([("de", 1), ("dé", 2), ("está", 3)]);
        assert_eq!(linker.lookup("de"), [1]);
        assert_eq!(linker.lookup("dé"), [2]);
        assert_eq!(linker.lookup("esta"), [3]);
        assert!(linker.lookup("nada").is_empty());
    }

    #[test]
    fn tokens_keep_accents_and_enye() {
        assert_eq!(
            tokens("¿Dónde está el niño?"),
            ["dónde", "está", "el", "niño"]
        );
    }

    #[test]
    fn picks_readable_sentences_only() {
        let linker = Linker::new([("yo", 1), ("tengo", 2), ("un", 3), ("perro", 4)]);
        let pairs = [
            Pair {
                es: "Yo tengo un perro.".into(),
                en: "I have a dog.".into(),
            },
            Pair {
                es: "Yo tengo un perro.".into(),
                en: "Duplicate.".into(),
            },
            Pair {
                es: "Yo tengo un xilófono azulado.".into(),
                en: "Too many unknown words.".into(),
            },
            Pair {
                es: "Perro.".into(),
                en: "Too short.".into(),
            },
        ];
        let picked = pick(&pairs, &linker);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].word_ids, [1, 2, 3, 4]);
    }
}
