//! Spanish definitions for the "Español primero" setting, from the
//! Spanish-language Wiktionary (es.wiktionary.org, via kaikki.org).
//!
//! These are monolingual dictionary definitions ("casa: Edificación
//! destinada a vivienda"), not translations, so they are taken as written:
//! the first sense of an entry with the word's part of speech that is
//! current, used in Spain and not a cross-reference to another form.

use serde::Deserialize;
use std::collections::HashMap;
use std::io::BufRead;

/// Longer definitions are cut at a word boundary and end in "…".
const MAX_CHARS: usize = 90;

/// Senses that aren't the plain, current, Peninsular meaning.
const SKIP_TAGS: &[&str] = &[
    "form-of",
    "no-gloss",
    "outdated",
    "obsolete",
    "archaic",
    "rare",
    "vulgar",
    "derogatory",
    "America",
    "Andes",
    "Argentina",
    "Bolivia",
    "Caribbean",
    "Central-America",
    "Chile",
    "Colombia",
    "Costa-Rica",
    "Cuba",
    "Dominican-Republic",
    "Ecuador",
    "El-Salvador",
    "Guatemala",
    "Honduras",
    "Mexico",
    "Nicaragua",
    "Panama",
    "Paraguay",
    "Peru",
    "Philippines",
    "Puerto-Rico",
    "Río-de-la-Plata",
    "US",
    "Uruguay",
    "Venezuela",
];

/// Glosses that only point at another word.
const POINTERS: &[&str] = &[
    "forma de",
    "forma del",
    "plural de",
    "femenino de",
    "masculino de",
    "variante de",
    "véase",
];

#[derive(Deserialize)]
struct Entry {
    word: String,
    #[serde(default)]
    lang_code: String,
    #[serde(default)]
    pos: String,
    #[serde(default)]
    senses: Vec<Sense>,
}

#[derive(Deserialize)]
struct Sense {
    #[serde(default)]
    glosses: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Spanish definitions for `wanted` (lemma → part of speech, as in the
/// English Wiktionary data), read from the es.wiktionary extract.
pub fn spanish_definitions(
    reader: impl BufRead,
    wanted: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut entries: HashMap<String, Vec<Entry>> = HashMap::new();
    for line in reader.lines() {
        let line = line.expect("read es.wiktionary dump");
        let Ok(e) = serde_json::from_str::<Entry>(&line) else {
            continue;
        };
        if e.lang_code == "es" && wanted.contains_key(&e.word) {
            entries.entry(e.word.clone()).or_default().push(e);
        }
    }
    entries
        .into_iter()
        .filter_map(|(word, es)| {
            let def = definition(&es, &wanted[&word])?;
            Some((word, def))
        })
        .collect()
}

/// The first usable sense among `entries` whose part of speech fits `pos`.
/// The Spanish Wiktionary lists "y" the letter before "y" the conjunction,
/// so the part of speech has to match.
fn definition(entries: &[Entry], pos: &str) -> Option<String> {
    entries
        .iter()
        .filter(|e| same_pos(pos, &e.pos))
        .flat_map(|e| &e.senses)
        .filter(|s| !s.tags.iter().any(|t| SKIP_TAGS.contains(&t.as_str())))
        .filter_map(|s| s.glosses.last().map(|g| clean(g)))
        .find(|g| {
            let lower = g.to_lowercase();
            g.chars().count() >= 3 && !POINTERS.iter().any(|p| lower.starts_with(p))
        })
}

fn same_pos(ours: &str, theirs: &str) -> bool {
    ours == theirs
        || matches!(
            (ours, theirs),
            ("det", "article" | "pron" | "adj")
                | ("article", "det")
                | ("contraction", "prep")
                | ("adj", "participle")
        )
}

/// Drops sense numbers ("mujer₁") and a leading scientific name
/// ("(Canis lupus familiaris) Variedad…"), and the closing full stop.
fn clean(gloss: &str) -> String {
    let no_marks: String = gloss.chars().filter(|c| !('₀'..='₉').contains(c)).collect();
    let mut s = no_marks.trim();
    if s.starts_with('(') {
        if let Some(end) = s.find(')') {
            s = s[end + 1..].trim_start();
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // the first sentence is the definition; later ones are usage notes
    let s = first_sentence(&s);
    let s = s.trim_end_matches('.').trim_end();
    if s.chars().count() <= MAX_CHARS {
        return s.to_string();
    }
    let cut: String = s.chars().take(MAX_CHARS).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end_matches([',', ';', ':']))
}

/// Up to the first ". " or ") " followed by a capital letter: where a new
/// sentence starts (Wikcionario sometimes leaves out the full stop after a
/// closing parenthesis).
fn first_sentence(s: &str) -> &str {
    for ((at, c), next) in s.char_indices().zip(s.chars().skip(1)) {
        if (c == '.' || c == ')') && next == ' ' {
            let rest = &s[at + c.len_utf8() + 1..];
            if rest.starts_with(|ch: char| ch.is_uppercase()) {
                return &s[..at + if c == ')' { 1 } else { 0 }];
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs(dump: &str, wanted: &[(&str, &str)]) -> HashMap<String, String> {
        let wanted = wanted
            .iter()
            .map(|(w, p)| (w.to_string(), p.to_string()))
            .collect();
        spanish_definitions(dump.as_bytes(), &wanted)
    }

    #[test]
    fn picks_the_entry_with_the_same_part_of_speech() {
        let dump = r#"{"word":"y","lang_code":"es","pos":"character","senses":[{"glosses":["Vigesimosexta letra del alfabeto español."]}]}
{"word":"y","lang_code":"es","pos":"conj","senses":[{"glosses":["Conjunción copulativa que une palabras."]}]}
{"word":"y","lang_code":"fr","pos":"conj","senses":[{"glosses":["Allí."]}]}"#;
        assert_eq!(
            defs(dump, &[("y", "conj")])["y"],
            "Conjunción copulativa que une palabras"
        );
    }

    #[test]
    fn skips_regional_obsolete_and_pointer_senses() {
        // one JSON object per line, as in the dump
        let senses = [
            r#"{"glosses":["Mantener relaciones sexuales."],"tags":["Mexico","vulgar"]}"#,
            r#"{"glosses":["Forma de algo antiguo."],"tags":["obsolete"]}"#,
            r#"{"glosses":["Forma del verbo acoger."]}"#,
            r#"{"glosses":["Asir, agarrar o tomar."]}"#,
        ];
        let dump = format!(
            r#"{{"word":"coger","lang_code":"es","pos":"verb","senses":[{}]}}"#,
            senses.join(",")
        );
        let dump = dump.as_str();
        assert_eq!(
            defs(dump, &[("coger", "verb")])["coger"],
            "Asir, agarrar o tomar"
        );
    }

    #[test]
    fn cleans_and_shortens_definitions() {
        assert_eq!(
            clean("(Canis lupus familiaris) Variedad doméstica del lobo."),
            "Variedad doméstica del lobo"
        );
        assert_eq!(
            clean("Mujer₁ casada con respecto de su marido."),
            "Mujer casada con respecto de su marido"
        );
        assert_eq!(
            clean("Contracción de la preposición a y el artículo el (masculino singular) Forma obligatoriamente contracta."),
            "Contracción de la preposición a y el artículo el (masculino singular)"
        );
        assert_eq!(
            clean("Señor Sr. de algo. no cortes aquí"),
            "Señor Sr. de algo. no cortes aquí"
        );
        let long = clean("Existir, hallarse alguien o algo con cierta permanencia y estabilidad en este o aquel lugar, situación o modo.");
        assert!(
            long.ends_with('…') && long.chars().count() <= MAX_CHARS + 1,
            "{long}"
        );
        assert!(!long.contains("  "));
    }

    #[test]
    fn words_without_a_fitting_entry_get_nothing() {
        let dump =
            r#"{"word":"sí","lang_code":"es","pos":"adv","senses":[{"glosses":["Afirmación."]}]}"#;
        assert!(defs(dump, &[("sí", "particle")]).is_empty());
    }
}
