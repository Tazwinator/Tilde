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

/// Longer definitions are shortened to about this length and end in "…".
const MAX_CHARS: usize = 90;

/// A definition only a little too long is kept whole: cutting it would save
/// a few words and lose its end.
const SLACK: usize = 10;

/// Words a shortened definition shouldn't end on, as they lead into the
/// part that was cut ("…en este o aquel…").
const DANGLING: &[&str] = &[
    "a", "al", "aquel", "aquella", "bajo", "como", "con", "cual", "cuando", "cuya", "cuyo", "de",
    "del", "desde", "donde", "durante", "e", "el", "en", "entre", "esa", "ese", "esta", "este",
    "hacia", "hasta", "la", "las", "le", "lo", "los", "mediante", "más", "menos", "muy", "ni", "o",
    "para", "por", "que", "se", "según", "sin", "sobre", "su", "sus", "tras", "u", "un", "una",
    "unas", "unos", "y",
];

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
    #[serde(default)]
    categories: Vec<serde_json::Value>,
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
/// so the part of speech has to match. A historical sense ("dinero: Antigua
/// moneda española…") gives way to a current one when there is one.
fn definition(entries: &[Entry], pos: &str) -> Option<String> {
    let mut usable = entries
        .iter()
        .filter(|e| same_pos(pos, &e.pos))
        .flat_map(|e| &e.senses)
        .filter(|s| !s.tags.iter().any(|t| SKIP_TAGS.contains(&t.as_str())))
        .filter_map(|s| Some((historical(s), clean(s.glosses.last()?))))
        .filter(|(_, g)| {
            let lower = g.to_lowercase();
            g.chars().count() >= 3 && !POINTERS.iter().any(|p| lower.starts_with(p))
        });
    let first = usable.next()?;
    if !first.0 {
        return Some(first.1);
    }
    Some(usable.find(|(old, _)| !old).unwrap_or(first).1)
}

fn historical(sense: &Sense) -> bool {
    let gloss = sense.glosses.last().map_or("", String::as_str);
    gloss.starts_with("Antigua ")
        || gloss.starts_with("Antiguo ")
        || sense
            .categories
            .iter()
            .any(|c| c.as_str() == Some("ES:Historia"))
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
    shorten(s.trim_end_matches('.').trim_end())
}

/// Cuts a long definition where it reads well: at the last clause break
/// (a comma, semicolon, colon, dash or bracket) in its second half, or else
/// after a whole word that doesn't lead into what was cut. A quote or
/// bracket left open is closed after the "…".
fn shorten(s: &str) -> String {
    if s.chars().count() <= MAX_CHARS + SLACK {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX_CHARS + 1).collect();
    let mut cut = head
        .rsplit_once(' ')
        .map_or(head.as_str(), |(words, _)| words);
    if let Some(at) = [", ", "; ", ": ", " (", " —"]
        .iter()
        .filter_map(|brk| cut.rfind(brk))
        .max()
    {
        if cut[..at].chars().count() >= MAX_CHARS / 2 {
            cut = &cut[..at];
        }
    }
    loop {
        cut = cut.trim_end_matches([',', ';', ':', ' ', '(', '—']);
        match cut.rsplit_once(' ') {
            Some((words, last)) if DANGLING.contains(&last.to_lowercase().as_str()) => cut = words,
            _ => break,
        }
    }
    let mut out = format!("{cut}…");
    if cut.matches('"').count() % 2 == 1 {
        out.push('"');
    }
    if cut.matches('(').count() > cut.matches(')').count() {
        out.push(')');
    }
    out
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
        assert_eq!(
            long,
            "Existir, hallarse alguien o algo con cierta permanencia y estabilidad…"
        );
    }

    #[test]
    fn long_definitions_are_cut_where_they_read_well() {
        // at the last clause break in the second half
        assert_eq!(
            shorten("Expresión de saludo utilizada entre dos o más personas de trato familiar, sin importar el momento del día"),
            "Expresión de saludo utilizada entre dos o más personas de trato familiar…"
        );
        // a bracket or quote the cut leaves open is closed
        assert_eq!(
            shorten("Regresar (llegar a un lugar de donde uno se había ido; invertir la dirección en que se venía moviendo)"),
            "Regresar (llegar a un lugar de donde uno se había ido…)"
        );
        assert_eq!(
            shorten("En comparación implícita o explícita, indica \"mayor cantidad de, exceso, aumento, ventaja o superioridad\""),
            "En comparación implícita o explícita, indica \"mayor cantidad de, exceso, aumento…\""
        );
        // a little over the limit is kept whole
        let near = "Período de siete días consecutivos que comienza el lunes y termina el domingo, en toda España";
        assert!(near.chars().count() > MAX_CHARS);
        assert_eq!(shorten(near), near);
    }

    #[test]
    fn a_current_sense_comes_before_a_historical_one() {
        let dump = r#"{"word":"dinero","lang_code":"es","pos":"noun","senses":[{"glosses":["Antigua moneda española de vellón."],"categories":["ES:Historia","ES:Monedas"]},{"glosses":["Bien que se usa para el pago de otros bienes y servicios."]}]}
{"word":"alquimia","lang_code":"es","pos":"noun","senses":[{"glosses":["Antigua práctica protocientífica."]}]}"#;
        let found = defs(dump, &[("dinero", "noun"), ("alquimia", "noun")]);
        assert_eq!(
            found["dinero"],
            "Bien que se usa para el pago de otros bienes y servicios"
        );
        assert_eq!(found["alquimia"], "Antigua práctica protocientífica");
    }

    #[test]
    fn words_without_a_fitting_entry_get_nothing() {
        let dump =
            r#"{"word":"sí","lang_code":"es","pos":"adv","senses":[{"glosses":["Afirmación."]}]}"#;
        assert!(defs(dump, &[("sí", "particle")]).is_empty());
    }
}
