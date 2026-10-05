//! Shared serde types used across the Tauri app, pipeline and (mirrored in TS)
//! by the frontend. Everything is camelCase on the wire.

use serde::{Deserialize, Serialize};

pub const TARGET_LANG: &str = "es";

// ---------------------------------------------------------------------------
// Profile / gamification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub xp: i64,
    pub level: i32,
    pub level_name: String,
    pub level_progress: f32,
    pub words_known: i64,
    pub words_learning: i64,
    pub minutes_total: i64,
    pub cefr_estimate: String,
    pub reviews_due: i64,
    pub new_words_ready: i64,
    pub days_since_last_session: i64,
    pub sessions_total: i64,
    pub badges_count: i64,
    pub definition_lang: String,
    pub tts_available: bool,
    pub target_lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelInfo {
    pub level: i32,
    pub name: String,
    pub xp_into_level: i64,
    pub xp_for_next: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Badge {
    pub id: String,
    pub name: String,
    pub description: String,
    pub earned_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Sessions & rounds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Quick,
    Standard,
    Deep,
    Sidecar,
    ReviewOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordCard {
    pub word_id: i64,
    pub lemma: String,
    pub pos: Option<String>,
    pub rank: i64,
    pub gloss_en: Option<String>,
    pub gloss_es: Option<String>,
    pub level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// `rename_all` only renames the variant tags; the fields inside each variant
// need `rename_all_fields` to come out camelCase like everything else.
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Round {
    /// Intro card shown the first time a new word is encountered.
    NewWord {
        id: i64,
        word: WordCard,
        example_es: Option<String>,
        example_en: Option<String>,
        audio_base64: Option<String>,
    },
    /// Multiple-choice: translate a word (either direction).
    Choice {
        id: i64,
        word_id: i64,
        prompt: String,
        prompt_lang: String,
        options: Vec<String>,
        answer_index: i32,
        audio_base64: Option<String>,
    },
    /// Match several ES words to EN glosses.
    Match {
        id: i64,
        pairs: Vec<MatchPair>,
    },
    /// Hear a word, pick the matching text.
    Listen {
        id: i64,
        word_id: i64,
        audio_base64: Option<String>,
        options: Vec<String>,
        answer_index: i32,
    },
    /// Hear a sentence, type what you hear (lenient grading).
    ListenType {
        id: i64,
        word_id: i64,
        sentence_es: String,
        audio_base64: Option<String>,
        answer: String,
    },
    /// Rebuild a Spanish sentence from shuffled tiles.
    Build {
        id: i64,
        word_id: i64,
        sentence_es: String,
        sentence_en: String,
        tiles: Vec<String>,
        answer: String,
    },
    /// Fill in the missing word in a sentence.
    Cloze {
        id: i64,
        word_id: i64,
        sentence_es: String,
        sentence_en: String,
        options: Vec<String>,
        answer_index: i32,
    },
    /// Conjugate a verb: "tú ___ (hablar, presente)".
    Conjugation {
        id: i64,
        word_id: i64,
        verb: String,
        tense: String,
        person: String,
        answer: String,
        hint: String,
    },
    /// Classic FSRS flashcard for due reviews.
    ReviewCard {
        id: i64,
        word_id: i64,
        es: String,
        en: String,
        example_es: Option<String>,
        example_en: Option<String>,
        audio_base64: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchPair {
    pub word_id: i64,
    pub es: String,
    pub en: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundResult {
    pub round_index: usize,
    pub correct: bool,
    pub duration_ms: i64,
    /// For ReviewCard rounds: FSRS grade 1=Again 2=Hard 3=Good 4=Easy.
    #[serde(default)]
    pub quality: Option<i32>,
    /// For Match rounds: the pairs the player got wrong at least once. The
    /// rest are graded as known.
    #[serde(default)]
    pub missed_word_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundFeedback {
    pub correct_answer: Option<String>,
    pub xp_gained: i32,
    pub combo: i32,
    pub level_up: Option<LevelInfo>,
    pub new_badges: Vec<Badge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStart {
    pub session_id: i64,
    pub kind: SessionKind,
    pub rounds: Vec<Round>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub xp: i64,
    pub rounds: i64,
    pub correct: i64,
    pub minutes: f64,
    pub new_words: Vec<String>,
    pub accuracy: f32,
    pub level_up: Option<LevelInfo>,
    pub new_badges: Vec<Badge>,
    pub best_combo: i32,
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementItem {
    pub word_id: i64,
    pub lemma: String,
    pub options: Vec<String>,
    pub answer_index: i32,
    pub band: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementAnswer {
    pub word_id: i64,
    pub correct: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementResult {
    pub frontier_rank: i64,
    pub words_marked_known: i64,
    pub cefr_estimate: String,
}

// ---------------------------------------------------------------------------
// Words, sentences, mining, stats
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordHit {
    pub word_id: i64,
    pub lemma: String,
    pub pos: Option<String>,
    pub rank: i64,
    pub gloss_en: Option<String>,
    pub gloss_es: Option<String>,
    pub level: String,
    pub known: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConjRow {
    pub tense: String,
    pub person: String,
    pub form: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordDetail {
    pub word: WordHit,
    pub audio_available: bool,
    pub card: Option<CardStateInfo>,
    pub conjugations: Option<Vec<ConjRow>>,
    pub sentences: Vec<ExSentence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExSentence {
    pub id: i64,
    pub es: String,
    pub en: Option<String>,
    pub mined: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardStateInfo {
    pub state: String,
    pub due_in_days: f64,
    pub reps: i64,
    pub lapses: i64,
    pub strength: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub file_title: String,
    pub sentences_found: i64,
    pub sentences_added: i64,
    pub words_matched: i64,
    pub top_words: Vec<WordHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinedSentence {
    pub id: i64,
    pub es: String,
    pub en: Option<String>,
    pub source: String,
    pub created_at: i64,
    pub in_deck: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPoint {
    pub label: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsData {
    pub minutes_per_week: Vec<SeriesPoint>,
    pub xp_per_week: Vec<SeriesPoint>,
    pub cumulative_words: Vec<SeriesPoint>,
    pub accuracy_all_time: f32,
    pub game_mix: Vec<SeriesPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub definition_lang: String,
    pub session_length: String,
    pub sound_enabled: bool,
    pub target_lang: String,
}

// ---------------------------------------------------------------------------
// TTS
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsInfo {
    pub engine: String,
    pub voice: String,
}

// ---------------------------------------------------------------------------
// Backups
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: String,
    pub name: String,
    /// "manual" | "auto" | "before-reset" | "before-restore"
    pub kind: String,
    /// Unix seconds.
    pub created_at: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFolder {
    pub dir: String,
    pub is_default: bool,
    /// Newest first.
    pub backups: Vec<BackupInfo>,
}
