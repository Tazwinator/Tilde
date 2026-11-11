pub mod types;

pub mod srt;

pub const APP_NAME: &str = "Tilde";

/// CEFR band derived from a word's frequency rank.
pub fn cefr_for_rank(rank: i64) -> &'static str {
    match rank {
        0..=1000 => "A1",
        1001..=2500 => "A2",
        2501..=5000 => "B1",
        5001..=8000 => "B2",
        _ => "C1",
    }
}

/// The key a word is searched by: lowercase, with accents, diaeresis and ñ
/// folded, so a query typed on any keyboard finds it. Answer checking keeps
/// ñ apart (grading.ts); this is only for lookup. The content database
/// stores these keys (`words.lemma_key`, `form_keys`), so a change here
/// needs a pipeline rebuild.
pub fn search_key(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' => 'a',
            'é' | 'è' => 'e',
            'í' | 'ì' => 'i',
            'ó' | 'ò' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// XP levels: every 500 XP, with fun Spanish-flavoured names.
pub const LEVEL_NAMES: [&str; 10] = [
    "Turista",
    "Aprendiz",
    "Callejero",
    "Charlatán",
    "Conversador",
    "Cuentista",
    "Bufón",
    "Bilingüe",
    "Maestro",
    "Leyenda",
];

pub fn level_for_xp(xp: i64) -> (i32, &'static str) {
    let level = (xp / 500) as i32;
    let name = LEVEL_NAMES[(level as usize).min(LEVEL_NAMES.len() - 1)];
    (level, name)
}

/// All badges the app can award, in a stable order.
pub fn all_badges() -> Vec<types::Badge> {
    let defs: &[(&str, &str, &str)] = &[
        ("first-session", "Primer Paso", "Termina tu primera sesión"),
        ("ten-sessions", "Frecuente", "Termina 10 sesiones"),
        ("fifty-sessions", "Veterano", "Termina 50 sesiones"),
        ("words-50", "Cincuentena", "Aprende 50 palabras"),
        ("words-200", "Doscientos", "Aprende 200 palabras"),
        ("words-500", "Quinientos", "Aprende 500 palabras"),
        ("words-1000", "¡Mil!", "Aprende 1000 palabras"),
        ("combo-10", "Racha de 10", "Encadena 10 aciertos seguidos"),
        ("combo-25", "Imparable", "Encadena 25 aciertos seguidos"),
        ("perfect-session", "Perfecta", "Termina una sesión sin un solo fallo"),
        ("minutes-60", "Una Hora", "Juega 1 hora en total"),
        ("minutes-300", "Cinco Horas", "Juega 5 horas en total"),
        ("minutes-900", "Quince Horas", "Juega 15 horas en total"),
        ("minutes-1800", "Treinta Horas", "Juega 30 horas en total"),
        ("conjugation-master", "Maestro de Verbos", "Acierta 100 conjugaciones"),
        ("first-mined", "Minero", "Mina tu primera frase de unos subtítulos"),
        ("mined-50", "Veta Rica", "Mina 50 frases"),
        ("sidecar-hour", "Multitarea", "Juega 1 hora en modo secundario"),
        ("return-trip", "De Vuelta", "Vuelve tras una semana fuera: sin culpa, solo progreso"),
        ("listener", "Oído Fino", "Acierta 200 rondas de escucha"),
    ];
    defs.iter()
        .map(|(id, name, description)| types::Badge {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            earned_at: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_key_folds_accents_and_enye() {
        assert_eq!(search_key("ÁRBOL"), "arbol");
        assert_eq!(search_key("Niño"), "nino");
        assert_eq!(search_key("pingüino"), "pinguino");
        assert_eq!(search_key("canté"), "cante");
        assert_eq!(search_key("está"), "esta");
        assert_eq!(search_key("áéíóúüñ ÁÉÍÓÚÜÑ"), "aeiouun aeiouun");
        // a grave accent typed by mistake still finds the word
        assert_eq!(search_key("àèìòù"), "aeiou");
        assert_eq!(search_key("tengo"), "tengo");
        // only search folds ñ: subtitle matching and grading keep it a letter
        assert_eq!(srt::strip_accents("niño"), "niño");
    }
}
