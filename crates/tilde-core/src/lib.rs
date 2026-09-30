pub mod types;

pub mod conjugator;
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
        ("first-session", "Primer Paso", "Finish your first session"),
        ("ten-sessions", "Frecuente", "Finish 10 sessions"),
        ("fifty-sessions", "Veterano", "Finish 50 sessions"),
        ("words-50", "Cincuentena", "Learn 50 words"),
        ("words-200", "Doscientos", "Learn 200 words"),
        ("words-500", "Quinientos", "Learn 500 words"),
        ("words-1000", "¡Mil!", "Learn 1000 words"),
        ("combo-10", "Racha de 10", "Get a 10-answer combo"),
        ("combo-25", "Imparable", "Get a 25-answer combo"),
        ("perfect-session", "Perfecta", "Finish a session with 100% accuracy"),
        ("minutes-60", "Una Hora", "Log 1 hour of play"),
        ("minutes-300", "Cinco Horas", "Log 5 hours of play"),
        ("minutes-900", "Quince Horas", "Log 15 hours of play"),
        ("minutes-1800", "Treinta Horas", "Log 30 hours of play"),
        ("conjugation-master", "Maestro de Verbos", "Answer 100 conjugations correctly"),
        ("first-mined", "Minero", "Mine your first sentence from subtitles"),
        ("mined-50", "Veta Rica", "Mine 50 sentences"),
        ("sidecar-hour", "Multitarea", "Log 1 hour in sidecar audio mode"),
        ("return-trip", "De Vuelta", "Come back after a week away — no guilt, just progress"),
        ("listener", "Oído Fino", "Answer 200 listening rounds correctly"),
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
