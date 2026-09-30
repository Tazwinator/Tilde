//! Spanish verb conjugation engine (Castilian, incl. vosotros).
//!
//! Persons (index 0-5): yo, tú, él/ella/usted, nosotros, vosotros,
//! ellos/ellas/ustedes. Tenses: presente, pretérito, imperfecto, futuro,
//! condicional, subjuntivo presente, subjuntivo imperfecto, imperativo,
//! gerundio, participio.

use crate::types::ConjRow;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const PERSONS: [&str; 6] = [
    "yo",
    "tú",
    "él/ella/usted",
    "nosotros",
    "vosotros",
    "ellos/ellas/ustedes",
];

const T_PRES: &str = "presente";
const T_PRET: &str = "pretérito";
const T_IMP: &str = "imperfecto";
const T_FUT: &str = "futuro";
const T_COND: &str = "condicional";
const T_SUBL: &str = "subjuntivo presente";
const T_SUBK: &str = "subjuntivo imperfecto";
const T_IMPER: &str = "imperativo";
const T_GER: &str = "gerundio";
const T_PART: &str = "participio";

#[derive(Clone, Copy, PartialEq)]
enum Stem {
    None,
    Eie,
    Oue,
    Ei,
    Uue,
}

struct Pattern {
    stem: Stem,
    /// -ir "o→ue" verbs: weak persons take o→u (durmamos, durmió).
    ir_oue: bool,
    /// irregular pretérito stem (u/i/j-stems, without endings)
    pret_stem: Option<&'static str>,
    /// j-stem pretérito: 3pl in -eron (dijeron, trajeron)
    pret_j: bool,
    fut_stem: Option<&'static str>,
    ger: Option<&'static str>,
    part: Option<&'static str>,
    /// explicit yo-form for presente (sé, doy, veo, quepo, he)
    yo: Option<&'static str>,
    /// explicit positive tú imperativo (di, haz, ten, ven, sal, pon, sé, ve)
    imp_tu: Option<&'static str>,
    /// full tense overrides: (tense, [6 forms; gerundio/participio use 1])
    over: &'static [(&'static str, [&'static str; 6])],
}

const DEFAULT: Pattern = Pattern {
    stem: Stem::None,
    ir_oue: false,
    pret_stem: None,
    pret_j: false,
    fut_stem: None,
    ger: None,
    part: None,
    yo: None,
    imp_tu: None,
    over: &[],
};

fn base() -> Pattern {
    Pattern {
        stem: Stem::None,
        ir_oue: false,
        pret_stem: None,
        pret_j: false,
        fut_stem: None,
        ger: None,
        part: None,
        yo: None,
        imp_tu: None,
        over: &[],
    }
}

fn apply_stem(stem: &str, class: Stem) -> String {
    match class {
        Stem::Eie => match stem.rfind('e') {
            Some(i) => format!("{}ie{}", &stem[..i], &stem[i + 1..]),
            None => stem.to_string(),
        },
        Stem::Oue => match stem.rfind('o') {
            Some(i) => format!("{}ue{}", &stem[..i], &stem[i + 1..]),
            None => stem.to_string(),
        },
        Stem::Ei => match stem.rfind('e') {
            Some(i) => format!("{}i{}", &stem[..i], &stem[i + 1..]),
            None => stem.to_string(),
        },
        Stem::Uue => match stem.rfind('u') {
            Some(i) => format!("{}ue{}", &stem[..i], &stem[i + 1..]),
            None => stem.to_string(),
        },
        Stem::None => stem.to_string(),
    }
}

fn accent_last_vowel(s: &str) -> String {
    for (i, c) in s.char_indices().rev() {
        let rep = match c {
            'a' => "á",
            'e' => "é",
            'i' => "í",
            'o' => "ó",
            'u' => "ú",
            _ => continue,
        };
        return format!("{}{}{}", &s[..i], rep, &s[i + c.len_utf8()..]);
    }
    s.to_string()
}

fn first_vowel_i(s: &str) -> Option<usize> {
    s.char_indices()
        .find(|(_, c)| "aeiouáéíóúü".contains(*c))
        .map(|(i, _)| i)
}

/// One-syllable-stem guard: for verbs like ir, ver, dar the "last vowel" of a
/// 1-2 char stem would mangle forms; those verbs are fully overridden anyway.
fn too_short_for_stem_change(stem: &str) -> bool {
    first_vowel_i(stem).is_none()
}

pub struct Conjugator;

impl Conjugator {
    pub fn is_verb_like(word: &str) -> bool {
        let w = word.to_lowercase();
        w.ends_with("arse") || w.ends_with("erse") || w.ends_with("irse")
            || w.ends_with("ar") || w.ends_with("er") || w.ends_with("ir")
            || w.ends_with("ár") || w.ends_with("ér") || w.ends_with("ír")
    }

    pub fn known_irregulars() -> Vec<&'static str> {
        patterns().keys().copied().collect()
    }

    pub fn is_irregular(verb: &str) -> bool {
        normalize(verb).map(|v| patterns().contains_key(v.as_str())).unwrap_or(false)
    }

    /// Conjugate a verb, returning rows ordered by tense then person.
    /// Also returns the stem change class via is_irregular/known_irregulars.
    pub fn conjugate(verb: &str) -> Option<Vec<ConjRow>> {
        let v = normalize(verb)?;
        let (inf, _reflexive) = split_reflexive(&v)?;
        if inf.chars().count() < 2 {
            return None;
        }
        let ending = if inf.ends_with("arse") || inf.ends_with("erse") || inf.ends_with("irse") {
            "ir"
        } else if inf.ends_with("ar") || inf.ends_with("ár") {
            "ar"
        } else if inf.ends_with("er") || inf.ends_with("ér") {
            "er"
        } else if inf.ends_with("ir") || inf.ends_with("ír") {
            "ir"
        } else {
            return None;
        };
        let stem = inf
            .strip_suffix(ending)
            .or_else(|| inf.strip_suffix("ír").filter(|_| ending == "ir"))
            .or_else(|| inf.strip_suffix("ér").filter(|_| ending == "er"))
            .or_else(|| inf.strip_suffix("ár").filter(|_| ending == "ar"))
            .unwrap();
        let pat = patterns().get(inf.as_str());
        if stem.is_empty() && pat.is_none() {
            return None;
        }

        let mut rows = Vec::new();
        push_simple(&mut rows, T_GER, &[gerund(stem, ending, pat)]);
        push_simple(&mut rows, T_PART, &[participle(stem, ending, pat)]);
        push6(&mut rows, T_PRES, &present(stem, ending, pat));
        push6(&mut rows, T_PRET, &preterite(stem, ending, pat));
        push6(&mut rows, T_IMP, &imperfect(stem, ending, pat));
        push6(&mut rows, T_FUT, &future(&inf, ending, pat));
        push6(&mut rows, T_COND, &conditional(&inf, ending, pat));
        push6(&mut rows, T_SUBL, &subjunctive(stem, ending, pat));
        push6(&mut rows, T_SUBK, &subjunctive_past(stem, ending, pat));
        push6(&mut rows, T_IMPER, &imperative(&inf, ending, pat));
        Some(rows)
    }
}

fn normalize(verb: &str) -> Option<String> {
    let v = verb.trim().to_lowercase();
    if v.is_empty() {
        return None;
    }
    Some(v)
}

fn split_reflexive(v: &str) -> Option<(String, bool)> {
    for suf in ["arse", "erse", "irse"] {
        if let Some(rest) = v.strip_suffix(suf) {
            return Some((format!("{}{}", rest, &suf[..2]), true));
        }
    }
    Some((v.to_string(), false))
}

fn over(pat: Option<&Pattern>, tense: &str) -> Option<[&'static str; 6]> {
    pat.and_then(|p| p.over.iter().find(|(t, _)| *t == tense)).map(|(_, f)| *f)
}

fn present(stem: &str, ending: &str, pat: Option<&Pattern>) -> [String; 6] {
    if let Some(f) = over(pat, T_PRES) {
        return f.iter().map(|s| s.to_string()).collect::<Vec<_>>().try_into().unwrap();
    }
    let p = pat.unwrap_or(&DEFAULT);
    let class = p.stem;

    let strong = |s: &str| apply_stem(s, class);
    let yo_form = if let Some(y) = p.yo {
        y.to_string()
    } else if class == Stem::Uue && ending == "ar" {
        format!("{}o", strong(stem))
    } else if let Some(weak) = yo_irregular(stem, ending, p, class) {
        weak
    } else if class != Stem::None && !too_short_for_stem_change(stem) {
        format!("{}o", strong(stem))
    } else {
        format!("{}o", stem)
    };

    let mono = stem.chars().count() <= 1;
    let end: [&str; 6] = match (ending, mono) {
        ("ar", false) => ["o", "as", "a", "amos", "áis", "an"],
        ("ar", true) => ["o", "as", "a", "amos", "ais", "an"],
        ("er", false) => ["o", "es", "e", "emos", "éis", "en"],
        ("er", true) => ["o", "es", "e", "emos", "eis", "en"],
        ("ir", false) => ["o", "es", "e", "imos", "ís", "en"],
        ("ir", true) => ["o", "es", "e", "imos", "is", "en"],
        _ => unreachable!(),
    };
    let mut out = [const { String::new() }; 6];
    out[0] = yo_form;
    for i in 1..6 {
        let s = if matches!(i, 1 | 2 | 5) && class != Stem::None && !too_short_for_stem_change(stem) {
            strong(stem)
        } else {
            stem.to_string()
        };
        out[i] = format!("{}{}", s, end[i]);
    }
    // -eer verbs (creer, leer): keep double e in strong persons
    if ending == "er" && stem.ends_with('e') && p.stem == Stem::None {
        out[1] = format!("{}e{}", stem, end[1]);
        out[2] = format!("{}e{}", stem, end[2]);
        out[5] = format!("{}e{}", stem, end[5]);
    }
    out
}

/// Irregular yo-forms beyond plain stem+o (go/zco/j-insertion etc.).
fn yo_irregular(stem: &str, ending: &str, p: &Pattern, class: Stem) -> Option<String> {
    // -uir verbs handled by overrides; -aer/-eer/-oír → igo
    if ending == "ir" && stem.ends_with("u") {
        return None; // construir etc. via overrides
    }
    if stem.ends_with("a") && ending == "er" {
        // caer: caigo
        return Some(format!("{}igo", stem));
    }
    if stem.ends_with("e") && ending == "er" {
        // leer: regular leo — not igo. Only -eer? "leer" is regular (leo, lees). traer is -aer.
        return None;
    }
    if stem.ends_with("o") && ending == "ir" {
        // oír: oigo
        return Some(format!("{}igo", stem));
    }
    // vowel + cer / vowel + cir → zco
    if ending == "er" && stem.ends_with("c") {
        let prev: Vec<char> = stem.chars().collect();
        if prev.len() >= 2 {
            let before = prev[prev.len() - 2];
            if "aeiouáéíóú".contains(before) {
                return Some(format!("{}zco", &stem[..stem.len() - 2]));
            } else {
                return Some(format!("{}zo", &stem[..stem.len() - 1]));
            }
        }
    }
    if ending == "ir" && stem.ends_with("c") {
        let prev: Vec<char> = stem.chars().collect();
        if prev.len() >= 2 {
            let before = prev[prev.len() - 2];
            if "aeiouáéíóú".contains(before) {
                return Some(format!("{}zco", &stem[..stem.len() - 2]));
            }
        }
    }
    // -ger/-gir → jo
    if (ending == "er" || ending == "ir") && stem.ends_with('g') {
        let prev: Vec<char> = stem.chars().collect();
        if prev.len() >= 2 {
            let before = prev[prev.len() - 2];
            if "aeiouáéíóú".contains(before) {
                return Some(format!("{}jo", &stem[..stem.len() - 1]));
            }
        }
    }
    // go-verbs are pattern-tagged via `go` field? we folded them into Pattern.go — see below
    let _ = p;
    let _ = class;
    None
}

fn preterite(stem: &str, ending: &str, pat: Option<&Pattern>) -> [String; 6] {
    if let Some(f) = over(pat, T_PRET) {
        return f.iter().map(|s| s.to_string()).collect::<Vec<_>>().try_into().unwrap();
    }
    let p = pat.unwrap_or(&DEFAULT);
    if let Some(st) = p.pret_stem {
        let st3 = if st == "hic" { "hiz" } else { st };
        let e3 = if p.pret_j { "eron" } else { "ieron" };
        let endings: [&str; 6] = ["e", "iste", "o", "imos", "isteis", e3];
        let mut out = [const { String::new() }; 6];
        for i in 0..6 {
            let s = if i == 2 { st3 } else { st };
            out[i] = format!("{}{}", s, endings[i]);
        }
        return out;
    }
    let class = p.stem;
    match ending {
        "ar" => {
            let e = ["é", "aste", "ó", "amos", "asteis", "aron"];
            std::array::from_fn(|i| format!("{}{}", stem, e[i]))
        }
        _ => {
            let e = ["í", "iste", "ió", "imos", "isteis", "ieron"];
            let mut out = [const { String::new() }; 6];
            let transform = |i: usize| -> String {
                if matches!(i, 2 | 5) && (p.ir_oue || class == Stem::Ei) {
                    if p.ir_oue {
                        apply_o_to_u(stem)
                    } else {
                        apply_stem(stem, Stem::Ei)
                    }
                } else {
                    stem.to_string()
                }
            };
            for i in 0..6 {
                out[i] = format!("{}{}", transform(i), e[i]);
            }
            out
        }
    }
}

fn apply_o_to_u(stem: &str) -> String {
    match stem.rfind('o') {
        Some(i) => format!("{}u{}", &stem[..i], &stem[i + 1..]),
        None => stem.to_string(),
    }
}

fn imperfect(stem: &str, ending: &str, pat: Option<&Pattern>) -> [String; 6] {
    if let Some(f) = over(pat, T_IMP) {
        return f.iter().map(|s| s.to_string()).collect::<Vec<_>>().try_into().unwrap();
    }
    match ending {
        "ar" => {
            let e = ["aba", "abas", "aba", "ábamos", "abais", "aban"];
            std::array::from_fn(|i| format!("{}{}", stem, e[i]))
        }
        _ => {
            let e = ["ía", "ías", "ía", "íamos", "íais", "ían"];
            std::array::from_fn(|i| format!("{}{}", stem, e[i]))
        }
    }
}

fn future(inf: &str, _ending: &str, pat: Option<&Pattern>) -> [String; 6] {
    if let Some(f) = over(pat, T_FUT) {
        return f.iter().map(|s| s.to_string()).collect::<Vec<_>>().try_into().unwrap();
    }
    let p = pat.unwrap_or(&DEFAULT);
    let stem = p.fut_stem.map(|s| s.to_string()).unwrap_or_else(|| inf.to_string());
    let e = ["é", "ás", "á", "emos", "éis", "án"];
    std::array::from_fn(|i| format!("{}{}", stem, e[i]))
}

fn conditional(inf: &str, _ending: &str, pat: Option<&Pattern>) -> [String; 6] {
    if let Some(f) = over(pat, T_COND) {
        return f.iter().map(|s| s.to_string()).collect::<Vec<_>>().try_into().unwrap();
    }
    let p = pat.unwrap_or(&DEFAULT);
    let stem = p.fut_stem.map(|s| s.to_string()).unwrap_or_else(|| inf.to_string());
    let e = ["ía", "ías", "ía", "íamos", "íais", "ían"];
    std::array::from_fn(|i| format!("{}{}", stem, e[i]))
}

fn subjunctive(stem: &str, ending: &str, pat: Option<&Pattern>) -> [String; 6] {
    if let Some(f) = over(pat, T_SUBL) {
        return f.iter().map(|s| s.to_string()).collect::<Vec<_>>().try_into().unwrap();
    }
    let p = pat.unwrap_or(&DEFAULT);
    let class = p.stem;

    // Strong stem: yo-form minus trailing "o" — handles go/zco/igo/doy etc.
    let pres = present(stem, ending, pat);
    let strong = pres[0]
        .strip_suffix('o')
        .map(|s| s.to_string())
        .unwrap_or_else(|| apply_stem(stem, class));

    // Weak stem for nosotros/vosotros:
    let regular_yo = format!("{}o", apply_stem(stem, class));
    let yo_dev = pres[0] != regular_yo;
    let weak = if ending == "ir" && class != Stem::None && !too_short_for_stem_change(stem) {
        match class {
            Stem::Oue => apply_o_to_u(stem),
            Stem::Eie | Stem::Ei => apply_stem(stem, Stem::Ei),
            Stem::Uue => format!("{}u", stem),
            Stem::None => stem.to_string(),
        }
    } else if p.yo.is_some() || yo_dev {
        strong.clone()
    } else {
        stem.to_string()
    };

    let end: [&str; 6] = if ending == "ar" {
        ["e", "es", "e", "emos", "éis", "en"]
    } else {
        ["a", "as", "a", "amos", "áis", "an"]
    };
    let mut out = [const { String::new() }; 6];
    for i in 0..6 {
        let s = if matches!(i, 3 | 4) {
            // nosotros/vosotros: orthographic fixes apply to weak stem
            fix_subj_ortho(&weak, ending)
        } else {
            fix_subj_ortho(&strong, ending)
        };
        out[i] = format!("{}{}", s, end[i]);
    }
    out
}

fn fix_subj_ortho(stem: &str, ending: &str) -> String {
    if ending == "ar" {
        if let Some(s) = stem.strip_suffix('c') {
            if stem.len() >= 2 {
                return format!("{}qu", s);
            }
        }
        if let Some(s) = stem.strip_suffix('g') {
            return format!("{}gu", s);
        }
        if let Some(s) = stem.strip_suffix('z') {
            return format!("{}c", s);
        }
    }
    stem.to_string()
}

fn subjunctive_past(stem: &str, ending: &str, pat: Option<&Pattern>) -> [String; 6] {
    if let Some(f) = over(pat, T_SUBK) {
        return f.iter().map(|s| s.to_string()).collect::<Vec<_>>().try_into().unwrap();
    }
    let p = pat.unwrap_or(&DEFAULT);
    let stem3 = if let Some(st) = p.pret_stem {
        if p.pret_j {
            format!("{}e", st)
        } else {
            format!("{}ie", st)
        }
    } else if ending == "ar" {
        format!("{}a", stem)
    } else {
        let pres3 = preterite(stem, ending, Some(p));
        let third = &pres3[5];
        third.strip_suffix("ron").unwrap_or(third).to_string()
    };
    let e = ["ra", "ras", "ra", "ramos", "rais", "ran"];
    let mut out = [const { String::new() }; 6];
    for i in 0..6 {
        let s = if i == 3 { accent_last_vowel(&stem3) } else { stem3.clone() };
        out[i] = format!("{}{}", s, e[i]);
    }
    out
}

fn imperative(inf: &str, ending: &str, pat: Option<&Pattern>) -> [String; 6] {
    if let Some(f) = over(pat, T_IMPER) {
        return f.iter().map(|s| s.to_string()).collect::<Vec<_>>().try_into().unwrap();
    }
    let p = pat.unwrap_or(&DEFAULT);
    let stem = inf
        .strip_suffix("ír")
        .or_else(|| inf.strip_suffix("ér"))
        .or_else(|| inf.strip_suffix("ár"))
        .or_else(|| inf.strip_suffix(ending))
        .unwrap_or(inf);
    let subj = subjunctive(stem, ending, pat);
    let pres = present(stem, ending, pat);
    let tu = p.imp_tu.map(|s| s.to_string()).unwrap_or_else(|| pres[2].clone());
    let vos = inf.strip_suffix('r').map(|s| format!("{}d", s)).unwrap_or_default();
    [
        String::new(),
        tu,
        subj[2].clone(),
        subj[3].clone(),
        vos,
        subj[5].clone(),
    ]
}

fn gerund(stem: &str, ending: &str, pat: Option<&Pattern>) -> String {
    if let Some(p) = pat {
        if let Some(g) = p.ger {
            return g.to_string();
        }
    }
    match ending {
        "ar" => format!("{}ando", stem),
        _ => {
            let p = pat.unwrap_or(&DEFAULT);
            if p.ir_oue {
                return format!("{}iendo", apply_o_to_u(stem));
            }
            if p.stem == Stem::Ei {
                return format!("{}iendo", apply_stem(stem, Stem::Ei));
            }
            if ending == "er" && (stem.ends_with('a') || stem.ends_with('e') || stem.ends_with('o')) {
                return format!("{}yendo", stem);
            }
            if ending == "ir" && (stem.ends_with('u') || stem.ends_with('o')) {
                return format!("{}yendo", stem);
            }
            format!("{}iendo", stem)
        }
    }
}

fn participle(stem: &str, ending: &str, pat: Option<&Pattern>) -> String {
    if let Some(p) = pat {
        if let Some(x) = p.part {
            return x.to_string();
        }
    }
    match ending {
        "ar" => format!("{}ado", stem),
        _ => format!("{}ido", stem),
    }
}

fn push6(rows: &mut Vec<ConjRow>, tense: &str, forms: &[String; 6]) {
    for (i, f) in forms.iter().enumerate() {
        if f.is_empty() {
            continue; // imperativo has no "yo"
        }
        rows.push(ConjRow {
            tense: tense.to_string(),
            person: PERSONS[i].to_string(),
            form: f.clone(),
        });
    }
}

fn push_simple(rows: &mut Vec<ConjRow>, tense: &str, forms: &[String]) {
    for f in forms {
        rows.push(ConjRow {
            tense: tense.to_string(),
            person: String::new(),
            form: f.clone(),
        });
    }
}

type Patterns = HashMap<&'static str, Pattern>;

fn patterns() -> &'static Patterns {
    static PATS: OnceLock<Patterns> = OnceLock::new();
    PATS.get_or_init(build_patterns)
}

fn build_patterns() -> Patterns {
    use Stem::*;
    let mut m: Patterns = HashMap::new();
    let mut add = |k: &'static str, p: Pattern| m.insert(k, p);

    // ---- fully irregular: ser, estar, ir, haber ----
    add("ser", Pattern { over: &[
        (T_PRES, ["soy", "eres", "es", "somos", "sois", "son"]),
        (T_PRET, ["fui", "fuiste", "fue", "fuimos", "fuisteis", "fueron"]),
        (T_IMP, ["era", "eras", "era", "éramos", "erais", "eran"]),
        (T_SUBL, ["sea", "seas", "sea", "seamos", "seáis", "sean"]),
        (T_SUBK, ["fuera", "fueras", "fuera", "fuéramos", "fuerais", "fueran"]),
        (T_IMPER, ["", "sé", "sea", "seamos", "sed", "sean"]),
    ], part: Some("sido"), ger: Some("siendo"), ..base() });

    add("estar", Pattern { over: &[
        (T_PRES, ["estoy", "estás", "está", "estamos", "estáis", "están"]),
        (T_PRET, ["estuve", "estuviste", "estuvo", "estuvimos", "estuvisteis", "estuvieron"]),
        (T_SUBL, ["esté", "estés", "esté", "estemos", "estéis", "estén"]),
        (T_SUBK, ["estuviera", "estuvieras", "estuviera", "estuviéramos", "estuvierais", "estuvieran"]),
        (T_IMPER, ["", "está", "esté", "estemos", "estad", "estén"]),
    ], fut_stem: Some("estar"), ger: Some("estando"), ..base() });

    add("ir", Pattern { over: &[
        (T_PRES, ["voy", "vas", "va", "vamos", "vais", "van"]),
        (T_PRET, ["fui", "fuiste", "fue", "fuimos", "fuisteis", "fueron"]),
        (T_IMP, ["iba", "ibas", "iba", "íbamos", "ibais", "iban"]),
        (T_SUBL, ["vaya", "vayas", "vaya", "vayamos", "vayáis", "vayan"]),
        (T_SUBK, ["fuera", "fueras", "fuera", "fuéramos", "fuerais", "fueran"]),
        (T_IMPER, ["", "ve", "vaya", "vayamos", "id", "vayan"]),
    ], ger: Some("yendo"), ..base() });

    add("haber", Pattern { over: &[
        (T_PRES, ["he", "has", "ha", "hemos", "habéis", "han"]),
        (T_PRET, ["hube", "hubiste", "hubo", "hubimos", "hubisteis", "hubieron"]),
        (T_IMP, ["había", "habías", "había", "habíamos", "habíais", "habían"]),
        (T_SUBL, ["haya", "hayas", "haya", "hayamos", "hayáis", "hayan"]),
        (T_SUBK, ["hubiera", "hubieras", "hubiera", "hubiéramos", "hubierais", "hubieran"]),
    ], fut_stem: Some("habr"), ..base() });

    // ---- major irregulars ----
    add("tener", Pattern { stem: Eie, pret_stem: Some("tuv"), fut_stem: Some("tendr"),
        over: &[(T_PRES, ["tengo", "tienes", "tiene", "tenemos", "tenéis", "tienen"])],
        imp_tu: Some("ten"), ..base() });

    add("hacer", Pattern { pret_stem: Some("hic"), pret_j: false, fut_stem: Some("har"),
        over: &[(T_PRES, ["hago", "haces", "hace", "hacemos", "hacéis", "hacen"])],
        imp_tu: Some("haz"), part: Some("hecho"), ..base() });

    add("poder", Pattern { stem: Oue, ir_oue: true, pret_stem: Some("pud"), fut_stem: Some("podr"),
        ger: Some("pudiendo"), ..base() });

    add("decir", Pattern { stem: Ei, pret_stem: Some("dij"), pret_j: true, fut_stem: Some("dir"),
        ger: Some("diciendo"), part: Some("dicho"),
        over: &[(T_PRES, ["digo", "dices", "dice", "decimos", "decís", "dicen"])],
        imp_tu: Some("di"), ..base() });

    add("venir", Pattern { stem: Ei, pret_stem: Some("vin"), fut_stem: Some("vendr"),
        ger: Some("viniendo"),
        over: &[(T_PRES, ["vengo", "vienes", "viene", "venimos", "venís", "vienen"])],
        imp_tu: Some("ven"), ..base() });

    add("querer", Pattern { stem: Eie, pret_stem: Some("quis"), fut_stem: Some("querr"), ..base() });

    add("saber", Pattern { yo: Some("sé"), pret_stem: Some("sup"), fut_stem: Some("sabr"),
        over: &[(T_SUBL, ["sepa", "sepas", "sepa", "sepamos", "sepáis", "sepan"])], ..base() });

    add("poner", Pattern { pret_stem: Some("pus"), fut_stem: Some("pondr"), part: Some("puesto"),
        over: &[(T_PRES, ["pongo", "pones", "pone", "ponemos", "ponéis", "ponen"])],
        imp_tu: Some("pon"), ..base() });

    add("salir", Pattern { fut_stem: Some("saldr"),
        over: &[(T_PRES, ["salgo", "sales", "sale", "salimos", "salís", "salen"])],
        imp_tu: Some("sal"), ..base() });

    add("traer", Pattern { pret_stem: Some("traj"), pret_j: true, ger: Some("trayendo"),
        over: &[(T_PRES, ["traigo", "traes", "trae", "traemos", "traéis", "traen"])], ..base() });

    add("oír", Pattern { ger: Some("oyendo"),
        over: &[(T_PRES, ["oigo", "oyes", "oye", "oímos", "oís", "oyen"]),
                (T_SUBL, ["oiga", "oigas", "oiga", "oigamos", "oigáis", "oigan"])], ..base() });

    add("caer", Pattern { ger: Some("cayendo"),
        over: &[(T_PRES, ["caigo", "caes", "cae", "caemos", "caéis", "caen"]),
                (T_SUBL, ["caiga", "caigas", "caiga", "caigamos", "caigáis", "caigan"])], ..base() });

    add("valer", Pattern { fut_stem: Some("valdr"),
        over: &[(T_PRES, ["valgo", "vales", "vale", "valemos", "valéis", "valen"]),
                (T_SUBL, ["valga", "valgas", "valga", "valgamos", "valgáis", "valgan"])], ..base() });

    add("caber", Pattern { yo: Some("quepo"), pret_stem: Some("cup"), fut_stem: Some("cabr"),
        over: &[(T_SUBL, ["quepa", "quepas", "quepa", "quepamos", "quepáis", "quepan"])], ..base() });

    add("andar", Pattern { pret_stem: Some("anduv"), fut_stem: Some("andr"),
        over: &[(T_SUBL, ["ande", "andes", "ande", "andemos", "andéis", "anden"])], ..base() });

    add("ver", Pattern { yo: Some("veo"), part: Some("visto"),
        over: &[(T_PRET, ["vi", "viste", "vio", "vimos", "visteis", "vieron"]),
                (T_IMP, ["veía", "veías", "veía", "veíamos", "veíais", "veían"]),
                (T_SUBL, ["vea", "veas", "vea", "veamos", "veáis", "vean"])], ..base() });

    add("dar", Pattern { yo: Some("doy"),
        over: &[(T_PRET, ["di", "diste", "dio", "dimos", "disteis", "dieron"]),
                (T_SUBL, ["dé", "des", "dé", "demos", "deis", "den"])], ..base() });

    add("jugar", Pattern { stem: Uue,
        over: &[(T_PRES, ["juego", "juegas", "juega", "jugamos", "jugáis", "juegan"]),
                (T_SUBL, ["juegue", "juegues", "juegue", "juguemos", "juguéis", "jueguen"])], ..base() });

    add("seguir", Pattern { stem: Ei,
        over: &[(T_PRES, ["sigo", "sigues", "sigue", "seguimos", "seguís", "siguen"]),
                (T_PRET, ["seguí", "seguiste", "siguió", "seguimos", "seguisteis", "siguieron"]),
                (T_SUBL, ["siga", "sigas", "siga", "sigamos", "sigáis", "sigan"])],
        ger: Some("siguiendo"), ..base() });

    add("conseguir", Pattern { stem: Ei,
        over: &[(T_PRES, ["consigo", "consigues", "consigue", "conseguimos", "conseguís", "consiguen"]),
                (T_PRET, ["conseguí", "conseguiste", "consiguió", "conseguimos", "conseguisteis", "consiguieron"]),
                (T_SUBL, ["consiga", "consigas", "consiga", "consigamos", "consigáis", "consigan"])],
        ger: Some("consiguiendo"), ..base() });

    add("dormir", Pattern { stem: Oue, ir_oue: true, ..base() });
    add("morir", Pattern { stem: Oue, ir_oue: true, part: Some("muerto"), ..base() });
    add("pedir", Pattern { stem: Ei, ..base() });
    add("servir", Pattern { stem: Ei, ..base() });
    add("repetir", Pattern { stem: Ei, ..base() });
    add("vestir", Pattern { stem: Ei, ..base() });
    add("medir", Pattern { stem: Ei, ..base() });
    add("reír", Pattern { stem: Ei,
        over: &[(T_PRES, ["río", "ríes", "ríe", "reímos", "reís", "ríen"]),
                (T_PRET, ["reí", "reíste", "rió", "reímos", "reísteis", "rieron"]),
                (T_SUBL, ["ría", "rías", "ría", "riamos", "riáis", "rían"])],
        ger: Some("riendo"), ..base() });

    add("preferir", Pattern { stem: Eie, ..base() });
    add("sentir", Pattern { stem: Eie, ir_oue: false, ..base() });
    add("divertir", Pattern { stem: Eie, ..base() });
    add("convertir", Pattern { stem: Eie, ..base() });
    add("pensar", Pattern { stem: Eie, ..base() });
    add("empezar", Pattern { stem: Eie, ..base() });
    add("comenzar", Pattern { stem: Eie, ..base() });
    add("cerrar", Pattern { stem: Eie, ..base() });
    add("entender", Pattern { stem: Eie, ..base() });
    add("defender", Pattern { stem: Eie, ..base() });
    add("perder", Pattern { stem: Eie, ..base() });
    add("quererse", Pattern { stem: Eie, pret_stem: Some("quis"), fut_stem: Some("querr"), ..base() });

    add("contar", Pattern { stem: Oue, ..base() });
    add("encontrar", Pattern { stem: Oue, ..base() });
    add("recordar", Pattern { stem: Oue, ..base() });
    add("acordar", Pattern { stem: Oue, ..base() });
    add("mostrar", Pattern { stem: Oue, ..base() });
    add("probar", Pattern { stem: Oue, ..base() });
    add("costar", Pattern { stem: Oue, ..base() });
    add("soñar", Pattern { stem: Oue, ..base() });
    add("almorzar", Pattern { stem: Oue, ..base() });
    add("volar", Pattern { stem: Oue, ..base() });
    add("aprobar", Pattern { stem: Oue, ..base() });
    add("sonar", Pattern { stem: Oue, ..base() });

    add("dormirse", Pattern { stem: Oue, ir_oue: true, ..base() });
    add("acostarse", Pattern { stem: Oue, ..base() });
    add("levantarse", Pattern { stem: None, ..base() });
    add("despertarse", Pattern { stem: Eie, ..base() });
    add("sentarse", Pattern { stem: Eie, ..base() });
    add("ponerse", Pattern { pret_stem: Some("pus"), fut_stem: Some("pondr"), part: Some("puesto"),
        over: &[(T_PRES, ["me pongo", "te pones", "se pone", "nos ponemos", "os ponéis", "se ponen"])],
        imp_tu: Some("ponte"), ..base() });

    add("conocer", Pattern {
        over: &[(T_PRES, ["conozco", "conoces", "conoce", "conocemos", "conocéis", "conocen"]),
                (T_SUBL, ["conozca", "conozcas", "conozca", "conozcamos", "conozcáis", "conozcan"])],
        ..base() });
    add("parecer", Pattern {
        over: &[(T_PRES, ["parezco", "pareces", "parece", "parecemos", "parecéis", "parecen"]),
                (T_SUBL, ["parezca", "parezcas", "parezca", "parezcamos", "parezcáis", "parezcan"])],
        ..base() });
    add("conducir", Pattern {
        over: &[(T_PRES, ["conduzco", "conduces", "conduce", "conducimos", "conducís", "conducen"]),
                (T_PRET, ["conduje", "condujiste", "condujo", "condujimos", "condujisteis", "condujeron"]),
                (T_SUBL, ["conduzca", "conduzcas", "conduzca", "conduzcamos", "conduzcáis", "conduzcan"])],
        ..base() });
    add("traducir", Pattern {
        over: &[(T_PRES, ["traduzco", "traduces", "traduce", "traducimos", "traducís", "traducen"]),
                (T_SUBL, ["traduzca", "traduzcas", "traduzca", "traduzcamos", "traduzcáis", "traduzcan"])],
        ..base() });
    add("producir", Pattern {
        over: &[(T_PRES, ["produzco", "produces", "produce", "producimos", "producís", "producen"]),
                (T_PRET, ["produje", "produjiste", "produjo", "produjimos", "produjisteis", "produjeron"]),
                (T_SUBL, ["produzca", "produzcas", "produzca", "produzcamos", "produzcáis", "produzcan"])],
        ..base() });

    add("construir", Pattern {
        over: &[(T_PRES, ["construyo", "construyes", "construye", "construimos", "construís", "construyen"]),
                (T_PRET, ["construí", "construiste", "construyó", "construimos", "construisteis", "construyeron"]),
                (T_SUBL, ["construya", "construyas", "construya", "construyamos", "construyáis", "construyan"])],
        ger: Some("construyendo"), ..base() });
    add("destruir", Pattern {
        over: &[(T_PRES, ["destruyo", "destruyes", "destruye", "destruimos", "destruís", "destruyen"]),
                (T_PRET, ["destruí", "destruiste", "destruyó", "destruimos", "destruisteis", "destruyeron"]),
                (T_SUBL, ["destruya", "destruyas", "destruya", "destruyamos", "destruyáis", "destruyan"])],
        ger: Some("destruyendo"), ..base() });
    add("huir", Pattern {
        over: &[(T_PRES, ["huyo", "huyes", "huye", "huimos", "huís", "huyen"]),
                (T_PRET, ["huí", "huiste", "huyó", "huimos", "huisteis", "huyeron"]),
                (T_SUBL, ["huya", "huyas", "huya", "huyamos", "huyáis", "huyan"])],
        ger: Some("huyendo"), ..base() });
    add("incluir", Pattern {
        over: &[(T_PRES, ["incluyo", "incluyes", "incluye", "incluimos", "incluís", "incluyen"]),
                (T_PRET, ["incluí", "incluiste", "incluyó", "incluimos", "incluisteis", "incluyeron"]),
                (T_SUBL, ["incluya", "incluyas", "incluya", "incluyamos", "incluyáis", "incluyan"])],
        ger: Some("incluyendo"), ..base() });

    add("abrir", Pattern { part: Some("abierto"), ..base() });
    add("cubrir", Pattern { part: Some("cubierto"), ..base() });
    add("descubrir", Pattern { part: Some("descubierto"), ..base() });
    add("escribir", Pattern { part: Some("escrito"), ..base() });
    add("describir", Pattern { part: Some("descrito"), ..base() });
    add("romper", Pattern { part: Some("roto"), ..base() });
    add("resolver", Pattern { part: Some("resuelto"), ..base() });
    add("volver", Pattern { part: Some("vuelto"), ..base() });
    add("devolver", Pattern { stem: Oue, part: Some("devuelto"), ..base() });
    add("envolver", Pattern { part: Some("envuelto"), ..base() });
    add("freír", Pattern { stem: Ei, part: Some("frito"), ger: Some("friendo"),
        over: &[(T_PRES, ["frío", "fríes", "fríe", "freímos", "freís", "fríen"])], ..base() });
    add("reñir", Pattern { stem: Ei, ger: Some("riñendo"),
        over: &[(T_PRES, ["riño", "riñes", "riñe", "reñimos", "reñís", "riñen"])], ..base() });

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms_of(verb: &str, tense: &str) -> Vec<String> {
        Conjugator::conjugate(verb)
            .unwrap()
            .into_iter()
            .filter(|r| r.tense == tense)
            .map(|r| r.form)
            .collect()
    }

    fn assert_forms(verb: &str, tense: &str, expected: &[&str]) {
        let got = forms_of(verb, tense);
        let exp: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        assert_eq!(got, exp, "{} in {}", verb, tense);
    }

    #[test]
    fn regular_hablar() {
        assert_forms("hablar", "presente", &["hablo", "hablas", "habla", "hablamos", "habláis", "hablan"]);
        assert_forms("hablar", "pretérito", &["hablé", "hablaste", "habló", "hablamos", "hablasteis", "hablaron"]);
        assert_forms("hablar", "imperfecto", &["hablaba", "hablabas", "hablaba", "hablábamos", "hablabais", "hablaban"]);
        assert_forms("hablar", "futuro", &["hablaré", "hablarás", "hablará", "hablaremos", "hablaréis", "hablarán"]);
        assert_forms("hablar", "condicional", &["hablaría", "hablarías", "hablaría", "hablaríamos", "hablaríais", "hablarían"]);
        assert_forms("hablar", "subjuntivo presente", &["hable", "hables", "hable", "hablemos", "habléis", "hablen"]);
        assert_forms("hablar", "subjuntivo imperfecto", &["hablara", "hablaras", "hablara", "habláramos", "hablarais", "hablaran"]);
        assert_forms("hablar", "imperativo", &["habla", "hable", "hablemos", "hablad", "hablen"]);
        assert_eq!(forms_of("hablar", "gerundio"), vec!["hablando"]);
        assert_eq!(forms_of("hablar", "participio"), vec!["hablado"]);
    }

    #[test]
    fn regular_comer_vivir() {
        assert_forms("comer", "presente", &["como", "comes", "come", "comemos", "coméis", "comen"]);
        assert_forms("comer", "subjuntivo presente", &["coma", "comas", "coma", "comamos", "comáis", "coman"]);
        assert_forms("vivir", "presente", &["vivo", "vives", "vive", "vivimos", "vivís", "viven"]);
        assert_forms("vivir", "pretérito", &["viví", "viviste", "vivió", "vivimos", "vivisteis", "vivieron"]);
        assert_eq!(forms_of("comer", "gerundio"), vec!["comiendo"]);
    }

    #[test]
    fn ser_estar_ir_haber() {
        assert_forms("ser", "presente", &["soy", "eres", "es", "somos", "sois", "son"]);
        assert_forms("ser", "pretérito", &["fui", "fuiste", "fue", "fuimos", "fuisteis", "fueron"]);
        assert_forms("ser", "imperfecto", &["era", "eras", "era", "éramos", "erais", "eran"]);
        assert_forms("ser", "subjuntivo presente", &["sea", "seas", "sea", "seamos", "seáis", "sean"]);
        assert_forms("ser", "imperativo", &["sé", "sea", "seamos", "sed", "sean"]);
        assert_forms("estar", "presente", &["estoy", "estás", "está", "estamos", "estáis", "están"]);
        assert_forms("estar", "pretérito", &["estuve", "estuviste", "estuvo", "estuvimos", "estuvisteis", "estuvieron"]);
        assert_forms("estar", "subjuntivo presente", &["esté", "estés", "esté", "estemos", "estéis", "estén"]);
        assert_forms("ir", "presente", &["voy", "vas", "va", "vamos", "vais", "van"]);
        assert_forms("ir", "imperfecto", &["iba", "ibas", "iba", "íbamos", "ibais", "iban"]);
        assert_forms("ir", "subjuntivo imperfecto", &["fuera", "fueras", "fuera", "fuéramos", "fuerais", "fueran"]);
        assert_eq!(forms_of("ir", "gerundio"), vec!["yendo"]);
        assert_forms("haber", "presente", &["he", "has", "ha", "hemos", "habéis", "han"]);
        assert_eq!(forms_of("haber", "participio"), vec!["habido"]);
    }

    #[test]
    fn tener_hacer_poder_querer() {
        assert_forms("tener", "presente", &["tengo", "tienes", "tiene", "tenemos", "tenéis", "tienen"]);
        assert_forms("tener", "pretérito", &["tuve", "tuviste", "tuvo", "tuvimos", "tuvisteis", "tuvieron"]);
        assert_forms("tener", "futuro", &["tendré", "tendrás", "tendrá", "tendremos", "tendréis", "tendrán"]);
        assert_forms("tener", "subjuntivo presente", &["tenga", "tengas", "tenga", "tengamos", "tengáis", "tengan"]);
        assert_forms("tener", "subjuntivo imperfecto", &["tuviera", "tuvieras", "tuviera", "tuviéramos", "tuvierais", "tuvieran"]);
        assert_forms("hacer", "presente", &["hago", "haces", "hace", "hacemos", "hacéis", "hacen"]);
        assert_forms("hacer", "pretérito", &["hice", "hiciste", "hizo", "hicimos", "hicisteis", "hicieron"]);
        assert_forms("hacer", "futuro", &["haré", "harás", "hará", "haremos", "haréis", "harán"]);
        assert_eq!(forms_of("hacer", "participio"), vec!["hecho"]);
        assert_forms("poder", "presente", &["puedo", "puedes", "puede", "podemos", "podéis", "pueden"]);
        assert_forms("poder", "pretérito", &["pude", "pudiste", "pudo", "pudimos", "pudisteis", "pudieron"]);
        assert_forms("poder", "subjuntivo presente", &["pueda", "puedas", "pueda", "podamos", "podáis", "puedan"]);
        assert_forms("poder", "subjuntivo imperfecto", &["pudiera", "pudieras", "pudiera", "pudiéramos", "pudierais", "pudieran"]);
        assert_forms("querer", "presente", &["quiero", "quieres", "quiere", "queremos", "queréis", "quieren"]);
        assert_forms("querer", "pretérito", &["quise", "quisiste", "quiso", "quisimos", "quisisteis", "quisieron"]);
        assert_forms("querer", "futuro", &["querré", "querrás", "querrá", "querremos", "querréis", "querrán"]);
    }

    #[test]
    fn decir_venir_dar_ver_saber() {
        assert_forms("decir", "presente", &["digo", "dices", "dice", "decimos", "decís", "dicen"]);
        assert_forms("decir", "pretérito", &["dije", "dijiste", "dijo", "dijimos", "dijisteis", "dijeron"]);
        assert_eq!(forms_of("decir", "gerundio"), vec!["diciendo"]);
        assert_eq!(forms_of("decir", "participio"), vec!["dicho"]);
        assert_forms("venir", "presente", &["vengo", "vienes", "viene", "venimos", "venís", "vienen"]);
        assert_forms("venir", "pretérito", &["vine", "viniste", "vino", "vinimos", "vinisteis", "vinieron"]);
        assert_eq!(forms_of("venir", "gerundio"), vec!["viniendo"]);
        assert_forms("dar", "presente", &["doy", "das", "da", "damos", "dais", "dan"]);
        assert_forms("dar", "pretérito", &["di", "diste", "dio", "dimos", "disteis", "dieron"]);
        assert_forms("dar", "subjuntivo presente", &["dé", "des", "dé", "demos", "deis", "den"]);
        assert_forms("ver", "presente", &["veo", "ves", "ve", "vemos", "veis", "ven"]);
        assert_forms("ver", "pretérito", &["vi", "viste", "vio", "vimos", "visteis", "vieron"]);
        assert_eq!(forms_of("ver", "participio"), vec!["visto"]);
        assert_forms("saber", "presente", &["sé", "sabes", "sabe", "sabemos", "sabéis", "saben"]);
        assert_forms("saber", "pretérito", &["supe", "supiste", "supo", "supimos", "supisteis", "supieron"]);
        assert_forms("saber", "subjuntivo presente", &["sepa", "sepas", "sepa", "sepamos", "sepáis", "sepan"]);
    }

    #[test]
    fn stem_changers_and_ortho() {
        assert_forms("dormir", "presente", &["duermo", "duermes", "duerme", "dormimos", "dormís", "duermen"]);
        assert_forms("dormir", "pretérito", &["dormí", "dormiste", "durmió", "dormimos", "dormisteis", "durmieron"]);
        assert_forms("dormir", "subjuntivo presente", &["duerma", "duermas", "duerma", "durmamos", "durmáis", "duerman"]);
        assert_eq!(forms_of("dormir", "gerundio"), vec!["durmiendo"]);
        assert_forms("pedir", "presente", &["pido", "pides", "pide", "pedimos", "pedís", "piden"]);
        assert_forms("pedir", "pretérito", &["pedí", "pediste", "pidió", "pedimos", "pedisteis", "pidieron"]);
        assert_eq!(forms_of("pedir", "gerundio"), vec!["pidiendo"]);
        assert_forms("empezar", "subjuntivo presente", &["empiece", "empieces", "empiece", "empecemos", "empecéis", "empiecen"]);
        assert_forms("buscar", "subjuntivo presente", &["busque", "busques", "busque", "busquemos", "busquéis", "busquen"]);
        assert_forms("llegar", "subjuntivo presente", &["llegue", "llegues", "llegue", "lleguemos", "lleguéis", "lleguen"]);
    }

    #[test]
    fn vosotros_and_misc() {
        let pres = forms_of("hablar", "presente");
        assert_eq!(pres[4], "habláis");
        let pres = forms_of("comer", "presente");
        assert_eq!(pres[4], "coméis");
        let pres = forms_of("vivir", "presente");
        assert_eq!(pres[4], "vivís");
        assert_forms("conocer", "presente", &["conozco", "conoces", "conoce", "conocemos", "conocéis", "conocen"]);
        assert_eq!(forms_of("traer", "gerundio"), vec!["trayendo"]);
        assert_eq!(forms_of("leer", "gerundio"), vec!["leyendo"]);
        assert_eq!(forms_of("oír", "gerundio"), vec!["oyendo"]);
        assert_eq!(forms_of("abrir", "participio"), vec!["abierto"]);
        assert_eq!(forms_of("escribir", "participio"), vec!["escrito"]);
        assert_eq!(forms_of("volver", "participio"), vec!["vuelto"]);
        assert!(Conjugator::is_verb_like("hablar"));
        assert!(Conjugator::is_verb_like("sentarse"));
        assert!(!Conjugator::is_verb_like("casa"));
    }
}

