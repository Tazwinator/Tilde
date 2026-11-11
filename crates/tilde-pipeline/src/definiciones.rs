//! Spanish definitions for the "Español primero" setting, from the
//! Spanish-language Wiktionary (es.wiktionary.org, via kaikki.org).
//!
//! These are monolingual dictionary definitions ("casa: Edificación
//! destinada a vivienda"), not translations, so they are taken as written:
//! the first sense of an entry with the word's part of speech that is
//! current, used in Spain (a sense that names Spain counts as Peninsular
//! whatever other regions it names) and not a cross-reference to another
//! form.
//!
//! The first such sense is often not the everyday one ("coche": a horse
//! carriage), and Wikcionario changes every week, so the commonest words
//! take their definition from `curation/definiciones.tsv` instead.

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

/// Senses that aren't the plain, current meaning. "derogatory" is not one
/// of them: Wikcionario puts it on the everyday sense of "tonto" or "viejo",
/// and a real slur is tagged vulgar as well.
const SKIP_TAGS: &[&str] = &[
    "form-of", "no-gloss", "outdated", "obsolete", "archaic", "rare", "vulgar",
];

/// Regions outside Spain: a sense tagged with one is skipped unless it names
/// Spain too. Other regional tags (South-Cone, Lunfardo…) are left alone,
/// because Wikcionario also puts them on plain senses ("auto": a car).
const REGION_TAGS: &[&str] = &[
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

/// Glosses that only point at another word ("Variante obsoleta de helada").
const POINTERS: &[&str] = &[
    "forma de",
    "forma del",
    "plural de",
    "femenino de",
    "masculino de",
    "variante ",
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

/// The reviewed definitions of `curation/definiciones.tsv`: lemma → its
/// definition, or `None` for a word that is to have no Spanish definition.
/// A line that isn't `lemma<TAB>definition` stops the build, and so does a
/// lemma listed twice: a file of thousands of rows mustn't lose one quietly.
pub fn curated(text: &str) -> HashMap<String, Option<String>> {
    let mut out = HashMap::new();
    for (n, line) in text.lines().enumerate() {
        // a line with a tab is a row, even when nothing else is on it
        if line.starts_with('#') || (!line.contains('\t') && line.trim().is_empty()) {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
        let [lemma, def] = fields[..] else {
            panic!(
                "definiciones.tsv line {}: not lemma<TAB>definition: {line:?}",
                n + 1
            );
        };
        if lemma.is_empty() || def.is_empty() {
            panic!(
                "definiciones.tsv line {}: empty lemma or definition: {line:?}",
                n + 1
            );
        }
        let def = (def != "-").then(|| def.to_string());
        if out.insert(lemma.to_string(), def).is_some() {
            panic!(
                "definiciones.tsv line {}: lemma listed twice: {line:?}",
                n + 1
            );
        }
    }
    out
}

/// The lemmas of `curated` that aren't in the word list (any more), sorted:
/// rows to look at after the list changes.
pub fn unused<'a>(
    curated: &'a HashMap<String, Option<String>>,
    wanted: &HashMap<String, String>,
) -> Vec<&'a str> {
    let mut unused: Vec<&str> = curated
        .keys()
        .filter(|lemma| !wanted.contains_key(*lemma))
        .map(String::as_str)
        .collect();
    unused.sort_unstable();
    unused
}

/// Spanish definitions for `wanted` (lemma → part of speech, as in the
/// English Wiktionary data). A word in `curated` gets what the file says,
/// exactly as written, whether or not Wikcionario has an entry for it; the
/// others are read from the es.wiktionary extract.
pub fn spanish_definitions(
    reader: impl BufRead,
    wanted: &HashMap<String, String>,
    curated: &HashMap<String, Option<String>>,
) -> HashMap<String, String> {
    let mut entries: HashMap<String, Vec<Entry>> = HashMap::new();
    for line in reader.lines() {
        let line = line.expect("read es.wiktionary dump");
        let Ok(e) = serde_json::from_str::<Entry>(&line) else {
            continue;
        };
        if e.lang_code == "es" && wanted.contains_key(&e.word) && !curated.contains_key(&e.word) {
            entries.entry(e.word.clone()).or_default().push(e);
        }
    }
    let mut found: HashMap<String, String> = entries
        .into_iter()
        .filter_map(|(word, es)| {
            let def = definition(&es, &wanted[&word])?;
            Some((word, def))
        })
        .collect();
    for (lemma, def) in curated {
        if let (true, Some(def)) = (wanted.contains_key(lemma), def) {
            found.insert(lemma.clone(), def.clone());
        }
    }
    found
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
        .filter(|s| !skipped(s))
        // Wikcionario's adjective tables, not definitions
        .filter(|s| !s.glosses.last().is_some_and(|g| g.contains('¦')))
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

/// An old or vulgar sense, or one used somewhere that isn't Spain. A sense
/// that names Spain is Peninsular whatever else it names ("vosotros": Spain,
/// the Philippines and Equatorial Guinea).
fn skipped(sense: &Sense) -> bool {
    let has = |tags: &[&str]| sense.tags.iter().any(|t| tags.contains(&t.as_str()));
    has(SKIP_TAGS) || (has(REGION_TAGS) && !has(&["Spain"]))
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
/// ("(Canis lupus familiaris) Variedad…"), wiki markup, and the closing
/// full stop.
fn clean(gloss: &str) -> String {
    let no_marks: String = gloss.chars().filter(|c| !('₀'..='₉').contains(c)).collect();
    let mut s = no_marks.trim();
    if s.starts_with('(') {
        if let Some(end) = s.find(')') {
            s = s[end + 1..].trim_start();
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = without_markup(&s);
    // the first sentence is the definition; later ones are usage notes
    let s = first_sentence(&s);
    shorten(s.trim_end_matches('.').trim_end())
}

/// Removes what Wikcionario's templates leave in a gloss:
/// - a ":*Sinónimos: …" list after the definition (cut there, with the
///   colon that led into it);
/// - footnote markers, "^([cita requerida])";
/// - references to another sense of the entry, "necio_(1–4)" and
///   "plástico(1)".
///
/// A bracket in running text ("el (masculino singular)") is none of these.
fn without_markup(s: &str) -> String {
    let s = match s.split_once(":*") {
        Some((definition, _)) => definition.trim_end().trim_end_matches(':'),
        None => s,
    };
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        match marker_len(rest, &out) {
            Some(len) => {
                // "…muy pronunciado ^([cita requerida])." and "la sombra
                // _(1–3)": a space before the marker goes with it
                out.truncate(out.trim_end().len());
                rest = &rest[len..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out.trim().to_string()
}

/// The length of the footnote marker or sense reference that `rest` starts
/// with, if it starts with one; `before` is the text it follows. A sense
/// reference is always attached to a word.
fn marker_len(rest: &str, before: &str) -> Option<usize> {
    let sense = rest.starts_with("_(") && before.trim_end().ends_with(char::is_alphabetic);
    if rest.starts_with("^(") || sense {
        // one that is never closed runs to the end
        return Some(closing_bracket(&rest[1..]).map_or(rest.len(), |at| at + 2));
    }
    if rest.starts_with('(') && before.ends_with(char::is_alphabetic) {
        let digits = rest[1..].bytes().take_while(u8::is_ascii_digit).count();
        return (digits > 0 && rest[1 + digits..].starts_with(')')).then_some(digits + 2);
    }
    None
}

/// The byte offset of the ")" that closes the "(" `s` starts with.
fn closing_bracket(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (at, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return Some(at);
        }
    }
    None
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

    fn wanted(words: &[(&str, &str)]) -> HashMap<String, String> {
        words
            .iter()
            .map(|(w, p)| (w.to_string(), p.to_string()))
            .collect()
    }

    fn defs(dump: &str, words: &[(&str, &str)]) -> HashMap<String, String> {
        spanish_definitions(dump.as_bytes(), &wanted(words), &HashMap::new())
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
    fn a_sense_that_names_spain_is_peninsular() {
        let dump = r#"{"word":"vosotros","lang_code":"es","pos":"pron","senses":[{"glosses":["Pronombre personal de segunda persona del plural."],"tags":["Equatorial-Guinea","Philippines","Spain","colloquial"]}]}
{"word":"coger","lang_code":"es","pos":"verb","senses":[{"glosses":["Agarrar algo."],"tags":["Colombia","Spain"]},{"glosses":["Recibir."]}]}
{"word":"polvo","lang_code":"es","pos":"noun","senses":[{"glosses":["Coito."],"tags":["Spain","vulgar"]},{"glosses":["Perra chica."],"tags":["Spain","outdated"]},{"glosses":["Tierra muy fina."]}]}
{"word":"auto","lang_code":"es","pos":"noun","senses":[{"glosses":["Automóvil."],"tags":["South-Cone"]},{"glosses":["Decreto judicial."]}]}"#;
        let found = defs(
            dump,
            &[
                ("vosotros", "pron"),
                ("coger", "verb"),
                ("polvo", "noun"),
                ("auto", "noun"),
            ],
        );
        assert_eq!(
            found["vosotros"],
            "Pronombre personal de segunda persona del plural"
        );
        assert_eq!(found["coger"], "Agarrar algo");
        // Spain doesn't excuse a vulgar or dated sense
        assert_eq!(found["polvo"], "Tierra muy fina");
        // a region that isn't in the list never skipped, and still doesn't
        assert_eq!(found["auto"], "Automóvil");
    }

    #[test]
    fn derogatory_senses_are_ordinary() {
        let dump = r#"{"word":"tonto","lang_code":"es","pos":"adj","senses":[{"glosses":["Falto de inteligencia o entendimiento."],"tags":["derogatory","noun"]},{"glosses":["Que, sin serlo, hace el papel de tonto₁ en circos."]}]}
{"word":"marica","lang_code":"es","pos":"noun","senses":[{"glosses":["Hombre homosexual."],"tags":["derogatory","vulgar"]},{"glosses":["Urraca."]}]}"#;
        let found = defs(dump, &[("tonto", "adj"), ("marica", "noun")]);
        assert_eq!(found["tonto"], "Falto de inteligencia o entendimiento");
        // a slur is tagged vulgar as well, and that still skips it
        assert_eq!(found["marica"], "Urraca");
    }

    #[test]
    fn inflection_tables_are_not_definitions() {
        let dump = r#"{"word":"cerrado","lang_code":"es","pos":"adj","senses":[{"glosses":["cerrado ¦ plural: cerrados ¦ femenino: cerrada ¦ femenino plural: cerradas"]}]}
{"word":"polo","lang_code":"es","pos":"noun","senses":[{"glosses":["polo ¦ plural: polos"]},{"glosses":["Cada uno de los dos extremos del eje de rotación de una esfera."]}]}"#;
        let found = defs(dump, &[("cerrado", "adj"), ("polo", "noun")]);
        assert!(!found.contains_key("cerrado"), "{found:?}");
        assert_eq!(
            found["polo"],
            "Cada uno de los dos extremos del eje de rotación de una esfera"
        );
    }

    #[test]
    fn variants_point_elsewhere() {
        let dump = r#"{"word":"hielo","lang_code":"es","pos":"noun","senses":[{"glosses":["Variante obsoleta de helada."]},{"glosses":["Estado sólido del agua."]}]}"#;
        assert_eq!(
            defs(dump, &[("hielo", "noun")])["hielo"],
            "Estado sólido del agua"
        );
    }

    #[test]
    fn wiki_markup_is_removed() {
        // a list of synonyms after the definition, and the colon that led into it
        assert_eq!(
            clean("Hacer que algo llegue a un destino o destinatario. :*Sinónimos: mandar, encomendar"),
            "Hacer que algo llegue a un destino o destinatario"
        );
        assert_eq!(
            clean(
                "Diseñar anticipadamente el plan para un proyecto o acción: :*Sinónimo: planificar"
            ),
            "Diseñar anticipadamente el plan para un proyecto o acción"
        );
        // footnote markers, wherever the full stop is
        assert_eq!(
            clean("Basto, ordinario, carente de refinamiento.^([definición imprecisa])"),
            "Basto, ordinario, carente de refinamiento"
        );
        assert_eq!(
            clean("Que tiene un sabor muy pronunciado ^([cita requerida])."),
            "Que tiene un sabor muy pronunciado"
        );
        assert_eq!(
            clean("Espejo ^([cita requerida]) de un armario. Otra frase."),
            "Espejo de un armario"
        );
        assert_eq!(clean("Sin cerrar ^([cita requerida"), "Sin cerrar");
        // references to another sense of the entry
        assert_eq!(
            clean("Dicho de una cosa, propia de alguien necio_(1–4)"),
            "Dicho de una cosa, propia de alguien necio"
        );
        assert_eq!(
            clean("Espacio que se encuentra a la sombra _(1–3)."),
            "Espacio que se encuentra a la sombra"
        );
        assert_eq!(
            clean("Longitud de cualquiera de los radios_(3,4) de un círculo."),
            "Longitud de cualquiera de los radios de un círculo"
        );
        assert_eq!(
            clean("Que esta hecho de plástico(1)"),
            "Que esta hecho de plástico"
        );
        assert_eq!(
            clean("Instrumento en forma de triángulo(2), que se hace sonar."),
            "Instrumento en forma de triángulo, que se hace sonar"
        );
        assert_eq!(
            clean("Dicho de una cosa, propia de alguien necio_(1–4"),
            "Dicho de una cosa, propia de alguien necio"
        );
        // ordinary brackets stay
        assert_eq!(
            clean("Contracción de la preposición a y el artículo el (masculino singular) Forma obligatoriamente contracta."),
            "Contracción de la preposición a y el artículo el (masculino singular)"
        );
        assert_eq!(
            clean("Vitamina B (12) y el número (3) de una lista"),
            "Vitamina B (12) y el número (3) de una lista"
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

    #[test]
    fn curated_definitions_win() {
        let dump = r#"{"word":"coche","lang_code":"es","pos":"noun","senses":[{"glosses":["Carruaje con dos o, más comúnmente, cuatro ruedas."]}]}
{"word":"planeta","lang_code":"es","pos":"noun","senses":[{"glosses":["Cuerpo celeste."]}]}
{"word":"casa","lang_code":"es","pos":"noun","senses":[{"glosses":["Edificación destinada a vivienda."]}]}"#;
        // longer than a definition from the dump is left, and with markup:
        // curated text is used as written
        let long = "Vehículo de cuatro ruedas(1), propulsado por un motor, destinado al transporte individual o familiar por carretera.";
        assert!(long.chars().count() > MAX_CHARS + SLACK);
        let curated = curated(&format!(
            "coche\t{long}\nplaneta\t-\ntuya\tDe ti\nlindar\tEstar contiguo\n"
        ));
        let words = wanted(&[
            ("coche", "noun"),
            ("planeta", "noun"),
            ("tuya", "noun"),
            ("casa", "noun"),
        ]);
        let found = spanish_definitions(dump.as_bytes(), &words, &curated);
        assert_eq!(found["coche"], long);
        // "-" takes the definition away, whatever the dump has
        assert!(!found.contains_key("planeta"));
        // the dump has no entry for it at all
        assert_eq!(found["tuya"], "De ti");
        // a word that isn't in the file is looked up as before
        assert_eq!(found["casa"], "Edificación destinada a vivienda");
        // a row for a word that left the list is ignored, and counted
        assert_eq!(found.len(), 3);
        assert_eq!(unused(&curated, &words), ["lindar"]);
    }

    #[test]
    fn curated_file_skips_comments_and_reads_a_dash_as_none() {
        let rows = curated(
            "# Reviewed definitions\n\n   \ncasa\tEdificación destinada a vivienda\r\nveces\t-\n",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows["casa"].as_deref(),
            Some("Edificación destinada a vivienda")
        );
        assert_eq!(rows["veces"], None);
        assert!(curated("# only a header\n").is_empty());
    }

    #[test]
    fn curated_file_rejects_malformed_lines() {
        for (bad, at) in [
            ("casa\tuno\ncasa\tdos", "line 2: lemma listed twice"),
            ("sin tabulador", "line 1: not lemma<TAB>definition"),
            ("# ok\ncasa\tuno\tdos", "line 2: not lemma<TAB>definition"),
            ("casa\t", "line 1: empty lemma or definition"),
            ("\tuno", "line 1: empty lemma or definition"),
            (
                "casa\tuno\n\nperro\t  \n",
                "line 3: empty lemma or definition",
            ),
            // a row with nothing in it is a lost row, not a blank line
            ("casa\tuno\n\t\n", "line 2: empty lemma or definition"),
            ("  \t  ", "line 1: empty lemma or definition"),
        ] {
            let panic = std::panic::catch_unwind(|| curated(bad)).expect_err(bad);
            let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(message.contains(at), "{bad:?}: {message}");
        }
    }

    /// What is wrong with a row of the curated file, if anything.
    fn flaw(lemma: &str, def: &str) -> Option<&'static str> {
        let subscript = |c: char| ('₀'..='₉').contains(&c);
        let lemma = lemma.to_lowercase();
        if def.chars().count() > MAX_CHARS + SLACK + 1 {
            Some("too long")
        } else if ["¦", ":*", "^(", "_(", "\n"].iter().any(|m| def.contains(m))
            || def.contains(subscript)
            // anything clean() would take out, such as "plástico(1)"
            || without_markup(def) != def
        {
            Some("wiki markup")
        } else if def.ends_with('.') {
            Some("ends with a full stop")
        } else if lemma.chars().count() >= 3
            && def
                .to_lowercase()
                .split(|c: char| !c.is_alphabetic())
                .any(|word| word == lemma)
        {
            // the definition is the prompt of rounds whose answer is the lemma
            Some("contains its own lemma")
        } else {
            None
        }
    }

    #[test]
    fn a_curated_definition_must_not_give_its_word_away() {
        let own = Some("contains its own lemma");
        assert_eq!(
            flaw("cobarde", "Propio o prototípico de una persona cobarde"),
            own
        );
        assert_eq!(flaw("ayer", "En el día de Ayer, el anterior a hoy"), own);
        // only as a whole word, and not for the shortest words
        assert_eq!(flaw("casa", "Lugar donde vive una pareja casada"), None);
        assert_eq!(flaw("de", "Indica de quién es algo"), None);

        assert_eq!(flaw("coche", &"a".repeat(102)), Some("too long"));
        assert_eq!(flaw("coche", &"ñ".repeat(101)), None);
        for markup in [
            "tabla ¦ plural: tablas",
            "Enviar :*Sinónimos: mandar",
            "Motivo ^([cita requerida])",
            "Mujer₁ casada",
            "Propia de alguien necio_(1–4)",
            "Hecho de plástico(1)",
        ] {
            assert_eq!(flaw("x", markup), Some("wiki markup"), "{markup}");
        }
        assert_eq!(
            flaw("al", "Contracción de a y el (masculino singular)"),
            None
        );
        assert_eq!(flaw("casa", "Edificación."), Some("ends with a full stop"));
    }

    #[test]
    fn the_curated_definitions_are_well_formed() {
        let rows = curated(include_str!("../curation/definiciones.tsv"));
        let mut flawed: Vec<String> = rows
            .iter()
            .filter_map(|(lemma, def)| {
                let def = def.as_deref()?;
                Some(format!("{lemma}: {} ({def})", flaw(lemma, def)?))
            })
            .collect();
        flawed.sort();
        assert!(flawed.is_empty(), "{}", flawed.join("\n"));
    }
}
