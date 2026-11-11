//! Session generation, scoring and gamification.

use crate::content::{gloss_parts, ContentDb, Example};
use crate::deck;
use crate::tts::Tts;
use rusqlite::Connection;
use std::collections::HashSet;
use tilde_core::types::{
    Badge, LevelInfo, MatchPair, PlacementAnswer, PlacementItem, PlacementResult, Round,
    SessionKind, SessionSummary, WordCard,
};

pub fn to_hit(w: &WordCard, known: bool, in_deck: bool) -> tilde_core::types::WordHit {
    tilde_core::types::WordHit {
        word_id: w.word_id,
        lemma: w.lemma.clone(),
        pos: w.pos.clone(),
        rank: w.rank,
        gloss_en: w.gloss_en.clone(),
        gloss_es: w.gloss_es.clone(),
        level: w.level.clone(),
        known,
        in_deck,
    }
}

/// The most one round adds to the time played. Long enough for a slow
/// dictation; short enough that a window left open on a round adds two
/// minutes, not the hour it sat there.
pub const ROUND_MS_CAP: i64 = 120_000;

/// A session nobody has answered for this long was abandoned (the window was
/// closed on it, the page reloaded) and is dropped when the next one starts.
/// Counted from the last answer, not the start, so a long session still
/// being played is never taken for one.
pub const STALE_S: f64 = 6.0 * 3600.0;

pub struct Session {
    pub kind: SessionKind,
    pub rounds: Vec<Round>,
    pub round_types: Vec<&'static str>,
    pub combo: i32,
    pub best_combo: i32,
    pub xp: i64,
    pub new_words: Vec<String>,
    /// Time spent on the rounds answered so far: the session's length. The
    /// rounds report it, so time away from the window is not play.
    pub active_ms: i64,
    /// When the session started or a round of it was last answered.
    pub last_active: f64,
    /// Answers given and how many were right. An intro card is shown, not
    /// answered, and counts in neither.
    pub correct: i64,
    pub total_graded: i64,
    /// This session's row in `events`, created on the first answer.
    pub event_id: Option<i64>,
    /// Round indices already graded; a repeat submit is ignored.
    pub answered: HashSet<usize>,
}

/// Everything grading a round changes in a `Session`, as it was before the
/// round. If the round can't be saved, `rewind` puts it back, so the session
/// in memory never counts an answer the database doesn't have. A field that
/// grading changes must be added here too.
pub struct SessionMark {
    combo: i32,
    best_combo: i32,
    xp: i64,
    active_ms: i64,
    correct: i64,
    total_graded: i64,
    new_words: usize,
    event_id: Option<i64>,
}

impl Session {
    pub fn mark(&self) -> SessionMark {
        SessionMark {
            combo: self.combo,
            best_combo: self.best_combo,
            xp: self.xp,
            active_ms: self.active_ms,
            correct: self.correct,
            total_graded: self.total_graded,
            new_words: self.new_words.len(),
            event_id: self.event_id,
        }
    }

    pub fn rewind(&mut self, mark: SessionMark) {
        self.combo = mark.combo;
        self.best_combo = mark.best_combo;
        self.xp = mark.xp;
        self.active_ms = mark.active_ms;
        self.correct = mark.correct;
        self.total_graded = mark.total_graded;
        self.new_words.truncate(mark.new_words);
        self.event_id = mark.event_id;
    }
}

pub fn round_count(kind: SessionKind) -> usize {
    match kind {
        SessionKind::Quick => 10,
        SessionKind::Standard => 18,
        SessionKind::Deep => 34,
        SessionKind::Sidecar => 12,
        SessionKind::ReviewOnly => 20,
    }
}

/// How many new words a session of this kind introduces (with an intro card)
/// when the player has enough words in the deck to fill its games.
fn new_word_count(kind: SessionKind) -> usize {
    match kind {
        SessionKind::ReviewOnly | SessionKind::Sidecar => 0,
        _ => (round_count(kind) / 5).max(2),
    }
}

/// What a mixed session of this kind aims for: (rounds, review cards, new
/// words, games).
fn shape(kind: SessionKind) -> (usize, usize, usize, usize) {
    let n = round_count(kind);
    let n_review = if kind == SessionKind::Quick { n / 3 } else { n * 2 / 5 };
    let n_new = new_word_count(kind);
    (n, n_review, n_new, n - n_review - n_new)
}

/// How many new words a session of this kind introduces for a player with
/// `learning` words in the deck. Games only ever quiz words the player has
/// met, so while the deck can't fill them the session introduces more words
/// instead, at most twice the usual number, and is otherwise a little
/// shorter. Home shows this number, so it and `gen_mixed` must both get it
/// from here.
pub fn planned_new_words(kind: SessionKind, learning: usize) -> usize {
    let (_, _, n_new, n_games) = shape(kind);
    n_new + n_new.min(n_games.saturating_sub(n_new + learning))
}

fn shuffle<T>(mut v: Vec<T>) -> Vec<T> {
    // deterministic-ish light shuffle (xorshift seeded by time)
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(42);
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for i in (1..v.len()).rev() {
        let j = (rnd() % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
    v
}

fn pick<T>(v: &[T], n: usize) -> Vec<T>
where
    T: Clone,
{
    shuffle(v.to_vec()).into_iter().take(n).collect()
}

/// The language a round about `w` shows meanings in. Spanish only if the
/// word has a Spanish definition; otherwise the whole round is in English.
/// A round never mixes the two: the one English gloss among Spanish
/// definitions is the answer, or is plainly not.
fn meaning_lang(w: &WordCard, definition_lang: &str) -> &'static str {
    if definition_lang == "es" && w.gloss_es.is_some() {
        "es"
    } else {
        "en"
    }
}

/// The word's meaning in the round's language (see `meaning_lang`).
fn gloss_in(w: &WordCard, lang: &str) -> String {
    let gloss = if lang == "es" { &w.gloss_es } else { &w.gloss_en };
    gloss.clone().unwrap_or_else(|| w.lemma.clone())
}

/// The word's meaning where it is shown on its own: in Spanish if the player
/// asked for that and there is a definition, else in English.
fn gloss_of(w: &WordCard, definition_lang: &str) -> String {
    gloss_in(w, meaning_lang(w, definition_lang))
}

/// What the option buttons of a multiple-choice round show, and how the
/// player gets the word they are asked about.
#[derive(Clone, Copy, PartialEq)]
enum Shown {
    /// Meanings, for a word the player reads.
    Meaning,
    /// Words, for a meaning the player reads.
    Word,
    /// Words, for a word the player only hears.
    Heard,
    /// Meanings, for a word the player only hears (sidecar).
    HeardMeaning,
}

/// How a word sounds, near enough: accents, a silent "h", "b"/"v" and
/// "ll"/"y" make no difference to the ear, so "sí" and "si", "hola" and
/// "ola", "valla" and "vaya" can't be told apart in a round that is only
/// heard. "ñ" is a sound of its own.
fn sound_key(word: &str) -> String {
    let mut key = String::new();
    let mut before = None;
    for c in tilde_core::srt::strip_accents(&word.to_lowercase()).chars() {
        match c {
            'h' if before != Some('c') => {}
            'v' => key.push('b'),
            c => key.push(c),
        }
        before = Some(c);
    }
    key.replace("ll", "y")
}

/// The options of a multiple-choice round about `w`, shuffled, and where the
/// right one is. Every round with options gets them here, so that all keep
/// the same promises: four options (wrong answers come from other word
/// classes when the word's own runs out), no two that read the same, and
/// none that is right as well:
/// - no wrong answer shares a meaning with the word (`distractor_candidates`);
/// - meanings are all in one language, and where a Spanish definition is
///   shown, no word is offered that it defines too;
/// - for a word that is only heard, nothing that sounds like it, or like
///   another option.
fn options_for(content: &ContentDb, w: &WordCard, shown: Shown, definition_lang: &str) -> (Vec<String>, i32) {
    options_from(w, content.distractor_candidates(w, 40), shown, definition_lang)
}

/// `options_for`, given the words to pick the wrong answers from, best first.
fn options_from(w: &WordCard, candidates: Vec<WordCard>, shown: Shown, definition_lang: &str) -> (Vec<String>, i32) {
    let lang = meaning_lang(w, definition_lang);
    let meanings = matches!(shown, Shown::Meaning | Shown::HeardMeaning);
    let heard = matches!(shown, Shown::Heard | Shown::HeardMeaning);
    // the Spanish definition, when the round shows it: on the buttons, or as
    // the prompt the words are picked for
    let definition = (lang == "es" && shown != Shown::Heard).then(|| gloss_parts(w.gloss_es.as_deref()));
    let text = |word: &WordCard| if meanings { gloss_in(word, lang) } else { word.lemma.clone() };
    let reads = |text: &str| text.trim().to_lowercase();

    let answer = text(w);
    let mut options = vec![answer.clone()];
    let mut read: HashSet<String> = HashSet::from([reads(&answer)]);
    let mut sounds: HashSet<String> = HashSet::from([sound_key(&w.lemma)]);
    for d in candidates {
        if options.len() == 4 {
            break;
        }
        if let Some(definition) = &definition {
            if meanings && d.gloss_es.is_none() {
                continue;
            }
            if !gloss_parts(d.gloss_es.as_deref()).is_disjoint(definition) {
                continue;
            }
        }
        let option = text(&d);
        if read.contains(&reads(&option)) || (heard && sounds.contains(&sound_key(&d.lemma))) {
            continue;
        }
        read.insert(reads(&option));
        sounds.insert(sound_key(&d.lemma));
        options.push(option);
    }
    let options = shuffle(options);
    // exact, since no two options read the same
    let answer_index = options.iter().position(|o| *o == answer).unwrap_or(0) as i32;
    (options, answer_index)
}

struct GenCtx<'a> {
    content: &'a ContentDb,
    user: &'a Connection,
    tts: &'a Tts,
    definition_lang: String,
    used_sentences: &'a mut HashSet<i64>,
    /// Words this session meets for the first time (it has their intro card).
    introduced: &'a mut HashSet<i64>,
    to_speak: std::cell::RefCell<Vec<String>>,
}

impl<'a> GenCtx<'a> {
    /// Audio that's already cached. Anything else is queued for synthesis in
    /// the background rather than made here: with Piper that's a process
    /// start per round, seconds per session, and the player would wait.
    fn audio(&self, text: &str) -> Option<String> {
        let cached = self.tts.cached(text);
        if cached.is_none() && self.tts.available() {
            self.to_speak.borrow_mut().push(text.to_string());
        }
        cached
    }
}

pub struct Generated {
    pub rounds: Vec<Round>,
    pub round_types: Vec<&'static str>,
    /// Texts the rounds will want spoken, in round order, not yet cached.
    pub to_speak: Vec<String>,
}

pub fn generate(
    kind: SessionKind,
    content: &ContentDb,
    user: &Connection,
    tts: &Tts,
    definition_lang: &str,
) -> Generated {
    let n = round_count(kind);
    let mut used_sentences = HashSet::new();
    let mut introduced = HashSet::new();
    let mut ctx = GenCtx {
        content,
        user,
        tts,
        definition_lang: definition_lang.to_string(),
        used_sentences: &mut used_sentences,
        introduced: &mut introduced,
        to_speak: Default::default(),
    };

    let (rounds, round_types) = match kind {
        SessionKind::Sidecar => gen_sidecar(&mut ctx, n),
        SessionKind::ReviewOnly => gen_reviews(&mut ctx, n),
        _ => gen_mixed(&mut ctx, kind),
    };
    // Generation order isn't play order (gen_mixed shuffles), so queue the
    // texts in the order the rounds will be played, each once.
    let queued: HashSet<String> = ctx.to_speak.take().into_iter().collect();
    let mut seen = HashSet::new();
    let to_speak = rounds
        .iter()
        .flat_map(spoken_texts)
        .filter(|t| queued.contains(t) && seen.insert(t.clone()))
        .collect();
    Generated { rounds, round_types, to_speak }
}

/// What the player will hear in a round (what `audio()` was asked for).
fn spoken_texts(round: &Round) -> Vec<String> {
    match round {
        Round::NewWord { word, .. } => vec![word.lemma.clone()],
        Round::Choice { prompt, prompt_lang, options, answer_index, .. } => {
            if prompt_lang == "es" {
                vec![prompt.clone()]
            } else {
                options.get(*answer_index as usize).cloned().into_iter().collect()
            }
        }
        Round::Listen { es, .. } => vec![es.clone()],
        Round::ListenType { sentence_es, .. } => vec![sentence_es.clone()],
        Round::ReviewCard { es, .. } => vec![es.clone()],
        _ => vec![],
    }
}

fn gen_reviews(ctx: &mut GenCtx, n: usize) -> (Vec<Round>, Vec<&'static str>) {
    let due = deck::due_card_ids(ctx.user, ctx.content.word_ids(), n);
    let mut rounds = Vec::new();
    let mut types = Vec::new();
    for (i, wid) in due.into_iter().enumerate() {
        if let Some(w) = ctx.content.word(wid) {
            let sentences = ctx.content.sentences_for_word(wid, 1);
            rounds.push(Round::ReviewCard {
                id: i as i64,
                word_id: wid,
                es: w.lemma.clone(),
                en: gloss_of(&w, &ctx.definition_lang),
                example_es: sentences.first().map(|s| s.es.clone()),
                example_en: sentences.first().and_then(|s| s.en.clone()),
                audio_base64: ctx.audio(&w.lemma),
            });
            types.push("review_card");
        }
    }
    (rounds, types)
}

fn gen_sidecar(ctx: &mut GenCtx, n: usize) -> (Vec<Round>, Vec<&'static str>) {
    // Only words the player has met. There are no intro cards here, with a
    // film on, and a quiz on a word never shown is a guess. With no cards
    // yet there are no rounds, and the page says to play a session first.
    let live = ctx.content.word_ids();
    let due = deck::due_card_ids(ctx.user, live, n);
    let learning = deck::learning_card_ids(ctx.user, live, n * 2);

    // due cards are also learning cards; each word once
    let mut seen = HashSet::new();
    let mut words: Vec<WordCard> = Vec::new();
    for wid in due.iter().chain(learning.iter()) {
        if seen.insert(*wid) {
            if let Some(w) = ctx.content.word(*wid) {
                words.push(w);
            }
        }
    }
    let words = pick(&words, n);

    let mut rounds = Vec::new();
    let mut types = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let id = i as i64;
        if i % 3 == 1 {
            if let Some((round, ty)) = dictation_round(ctx, id, w) {
                rounds.push(round);
                types.push(ty);
                continue;
            }
        }
        // a round that shows no word needs a voice to say it
        if i % 3 == 2 && ctx.tts.available() {
            let (options, answer_index) = options_for(ctx.content, w, Shown::HeardMeaning, &ctx.definition_lang);
            rounds.push(Round::Listen {
                id,
                word_id: w.word_id,
                es: w.lemma.clone(),
                audio_base64: ctx.audio(&w.lemma),
                options,
                answer_index,
            });
            types.push("listen");
        } else {
            let (options, answer_index) = options_for(ctx.content, w, Shown::Meaning, &ctx.definition_lang);
            rounds.push(Round::Choice {
                id,
                word_id: w.word_id,
                prompt: w.lemma.clone(),
                prompt_lang: "es".into(),
                options,
                answer_index,
                audio_base64: ctx.audio(&w.lemma),
            });
            types.push("choice");
        }
    }
    (rounds, types)
}

fn gen_mixed(ctx: &mut GenCtx, kind: SessionKind) -> (Vec<Round>, Vec<&'static str>) {
    let (n, n_review, _, n_games) = shape(kind);
    let live = ctx.content.word_ids();

    // gather pools
    let due = deck::due_card_ids(ctx.user, live, n_review);
    // words to play games with: the ones in the deck
    let learn_words: Vec<WordCard> = deck::learning_card_ids(ctx.user, live, n * 2)
        .iter()
        .filter_map(|id| ctx.content.word(*id))
        .collect();
    let carded = deck::card_ids(ctx.user);
    let new_words = ctx
        .content
        .new_candidates(planned_new_words(kind, learn_words.len()), &carded);

    let mut rounds: Vec<Round> = Vec::new();
    let mut types: Vec<&'static str> = Vec::new();
    let mut rid: i64 = 0;

    // intro cards for new words
    let mut intro_ids: HashSet<i64> = HashSet::new();
    let mut game_words: Vec<WordCard> = Vec::new();

    // new word blocks: intro + a first quiz. The card is created when the
    // intro is answered, so an abandoned session leaves no half-seen words
    // behind.
    for w in &new_words {
        if intro_ids.insert(w.word_id) {
            let sentences = ctx.content.sentences_for_word(w.word_id, 1);
            rounds.push(Round::NewWord {
                id: rid,
                word: w.clone(),
                example_es: sentences.first().map(|s| s.es.clone()),
                example_en: sentences.first().and_then(|s| s.en.clone()),
                audio_base64: ctx.audio(&w.lemma),
            });
            types.push("new_word");
            rid += 1;
            ctx.introduced.insert(w.word_id);

            // first quiz right after intro: es -> en choice
            let (round, ty) = choice_round(ctx, rid, w);
            rounds.push(round);
            types.push(ty);
            rid += 1;
            game_words.push(w.clone());
        }
    }

    // Games are played with the words just introduced and the ones in the
    // deck, and no others: a quiz on a word the player was never shown is a
    // guess, and its answer would be graded as if it weren't. A deck too
    // small to fill the games makes a shorter session.
    let game_pool: Vec<WordCard> = {
        let mut all = game_words.clone();
        all.extend(learn_words);
        all
    };

    // conjugation rounds for verbs
    let verbs = ctx.content.verbs(60);
    let mut conj_used = 0usize;
    let max_conj = (n_games / 6).max(1);
    let mut dictations = 0usize;
    let max_dictations = (n_games / 6).max(1);

    for w in pick(&game_pool, n_games) {
        let sentences = ctx.content.sentences_for_word(w.word_id, 4);
        let usable: Vec<&Example> = sentences.iter().filter(|s| !ctx.used_sentences.contains(&s.id)).collect();

        let kind_chosen = (rid as usize) % 4;
        if conj_used < max_conj
            && ctx.content.is_verb(w.word_id)
            && verbs.iter().any(|v| v.word_id == w.word_id)
            && kind_chosen == 3
        {
            if let Some((round, ty)) = conjugation_round(ctx, rid, &w) {
                rounds.push(round);
                types.push(ty);
                rid += 1;
                conj_used += 1;
                continue;
            }
        }
        match kind_chosen {
            0 => {
                let (round, ty) = choice_round(ctx, rid, &w);
                rounds.push(round);
                types.push(ty);
            }
            1 => {
                // A whole sentence by ear is too much for a word first seen
                // minutes ago, and one or two a session are plenty.
                let dictation = if dictations < max_dictations && !ctx.introduced.contains(&w.word_id) {
                    dictation_round(ctx, rid, &w)
                } else {
                    None
                };
                dictations += usize::from(dictation.is_some());
                let (round, ty) = dictation.unwrap_or_else(|| listen_round(ctx, rid, &w));
                rounds.push(round);
                types.push(ty);
            }
            2 => {
                let build = usable.iter().find_map(|s| build_tiles(&s.es).map(|tiles| (s, tiles)));
                if let Some((s, tiles)) = build {
                    ctx.used_sentences.insert(s.id);
                    rounds.push(Round::Build {
                        id: rid,
                        word_id: w.word_id,
                        sentence_es: s.es.clone(),
                        sentence_en: s.en.clone().unwrap_or_default(),
                        tiles,
                        answer: s.es.clone(),
                    });
                    types.push("build");
                } else {
                    let (round, ty) = choice_round(ctx, rid, &w);
                    rounds.push(round);
                    types.push(ty);
                }
            }
            _ => {
                let cloze = usable.iter().find_map(|s| cloze_round(ctx, rid, &w, s).map(|r| (s.id, r)));
                let (round, ty) = match cloze {
                    Some((sid, round)) => {
                        ctx.used_sentences.insert(sid);
                        round
                    }
                    None => listen_round(ctx, rid, &w),
                };
                rounds.push(round);
                types.push(ty);
            }
        }
        rid += 1;
    }

    // match rounds (one or two)
    for chunk in match_chunks(&game_pool, &ctx.definition_lang, (n_games / 6).min(2)) {
        rounds.push(Round::Match {
            id: rid,
            pairs: chunk
                .iter()
                .map(|w| MatchPair {
                    word_id: w.word_id,
                    es: w.lemma.clone(),
                    en: gloss_in(w, &ctx.definition_lang),
                })
                .collect(),
        });
        types.push("match");
        rid += 1;
    }

    // review cards
    for (i, wid) in due.into_iter().take(n_review).enumerate() {
        if let Some(w) = ctx.content.word(wid) {
            let sentences = ctx.content.sentences_for_word(wid, 1);
            rounds.push(Round::ReviewCard {
                id: rid + i as i64,
                word_id: wid,
                es: w.lemma.clone(),
                en: gloss_of(&w, &ctx.definition_lang),
                example_es: sentences.first().map(|s| s.es.clone()),
                example_en: sentences.first().and_then(|s| s.en.clone()),
                audio_base64: ctx.audio(&w.lemma),
            });
            types.push("review_card");
        }
    }

    // interleave: shuffle the games but keep intro blocks near start
    let intro_len = intro_ids.len() * 2;
    let (head, tail) = rounds.split_at(intro_len.min(rounds.len()));
    let head = head.to_vec();
    let tail = tail.to_vec();
    // head keeps intro+first-quiz order; tail shuffled
    let tail_types = types.split_at(intro_len.min(types.len())).1.to_vec();
    let pairs: Vec<(Round, &'static str)> =
        shuffle(tail.into_iter().zip(tail_types).collect());
    let mut out_rounds = head.to_vec();
    let mut out_types = types[..intro_len.min(types.len())].to_vec();
    for (r, t) in pairs {
        out_rounds.push(r);
        out_types.push(t);
    }
    (out_rounds, out_types)
}

fn choice_round(ctx: &mut GenCtx, id: i64, w: &WordCard) -> (Round, &'static str) {
    // odd rounds go from the meaning to the word
    let (prompt, prompt_lang, shown) = if id % 2 == 1 {
        (gloss_of(w, &ctx.definition_lang), "en", Shown::Word)
    } else {
        (w.lemma.clone(), "es", Shown::Meaning)
    };
    let (options, answer_index) = options_for(ctx.content, w, shown, &ctx.definition_lang);
    (
        Round::Choice {
            id,
            word_id: w.word_id,
            prompt,
            prompt_lang: prompt_lang.into(),
            options,
            answer_index,
            audio_base64: ctx.audio(&w.lemma),
        },
        "choice",
    )
}

/// Hear the word, pick it out. Without a speech engine there is nothing to
/// hear, so the word is quizzed in writing instead.
fn listen_round(ctx: &mut GenCtx, id: i64, w: &WordCard) -> (Round, &'static str) {
    if !ctx.tts.available() {
        return choice_round(ctx, id, w);
    }
    let (options, answer_index) = options_for(ctx.content, w, Shown::Heard, &ctx.definition_lang);
    (
        Round::Listen {
            id,
            word_id: w.word_id,
            es: w.lemma.clone(),
            audio_base64: ctx.audio(&w.lemma),
            options,
            answer_index,
        },
        "listen",
    )
}

/// The longest sentence given as a dictation, in characters. It has to be
/// held in the ear and typed out exactly. Common words rarely lack one this
/// short among the four example sentences looked at: fewer than twenty of
/// the 2,000 most frequent that are taught do.
const DICTATION_MAX_CHARS: usize = 40;

/// Hear a sentence with the word in it, write it down. `None` when there is
/// no speech engine (nothing could be heard) or no short sentence that this
/// session hasn't used yet.
fn dictation_round(ctx: &mut GenCtx, id: i64, w: &WordCard) -> Option<(Round, &'static str)> {
    if !ctx.tts.available() {
        return None;
    }
    let Example { id: sid, es, .. } = ctx.content.sentences_for_word(w.word_id, 4).into_iter().find(|s| {
        !ctx.used_sentences.contains(&s.id) && s.es.chars().count() <= DICTATION_MAX_CHARS
    })?;
    ctx.used_sentences.insert(sid);
    Some((
        Round::ListenType {
            id,
            word_id: w.word_id,
            audio_base64: ctx.audio(&es),
            answer: es.clone(),
            sentence_es: es,
        },
        "listen_type",
    ))
}

/// Blanks the word as it appears in the sentence ("comió", not "comer") and
/// offers similar words in the same form as distractors. `None` when the
/// sentence can't make a fair question of it.
fn cloze_round(ctx: &mut GenCtx, id: i64, w: &WordCard, example: &Example) -> Option<(Round, &'static str)> {
    let es = example.es.as_str();
    // The translation is shown with the gap: often more than one of the
    // options makes a good Spanish sentence, and only it says which is meant.
    let en = example.en.as_deref().map(str::trim).filter(|en| !en.is_empty())?;
    // The gap is the token the sentence was linked to the word by. The first
    // token that is spelled like a form of the word may be another word: in
    // "El policía pitó al coche para que parara", an example of "parar",
    // that is "para".
    let target = example.form.as_str();
    let (start, token) = words_in(es).into_iter().find(|(_, token)| token.to_lowercase() == target)?;
    let forms = ctx.content.forms_of(w.word_id);
    // The form to put the distractors in. A spelling that is listed with the
    // word without a place in its tables ("dame", "creerlo") has none: the
    // others would stand next to it as bare infinitives, and the only option
    // that looks like the gap would be the answer.
    let tag = if target == w.lemma.to_lowercase() {
        ""
    } else {
        forms.iter().find(|(form, tag)| form == target && !tag.is_empty()).map(|(_, tag)| tag.as_str())?
    };
    let said = spaced_words(en);
    let mut options = vec![target.to_string()];
    for d in ctx.content.similar_words(w, 12) {
        if options.len() == 4 {
            break;
        }
        // A word the translation names fits the gap as well as the answer
        // does: "completamente" where it says "completely" for "totalmente".
        // (A meaning with a note in brackets is not something a sentence says.)
        let named = gloss_parts(d.gloss_en.as_deref()).iter().any(|meaning| {
            let phrase = spaced_words(meaning);
            !meaning.contains('(') && !phrase.trim().is_empty() && said.contains(&phrase)
        });
        if named {
            continue;
        }
        if let Some(f) = ctx.content.form_with_tag(d.word_id, tag) {
            if !options.contains(&f) {
                options.push(f);
            }
        }
    }
    if options.len() < 3 {
        return None;
    }
    let blanked = format!("{}___{}", &es[..start], &es[start + token.len()..]);
    let opts = shuffle(options);
    let answer_index = opts.iter().position(|o| *o == target).unwrap_or(0) as i32;
    Some((
        Round::Cloze {
            id,
            word_id: w.word_id,
            sentence_es: blanked,
            sentence_en: en.to_string(),
            options: opts,
            answer_index,
        },
        "cloze",
    ))
}

/// `s` in lowercase with one space around every word and nothing else
/// between them, so that looking for " word " or " two words " in it finds
/// whole words only.
fn spaced_words(s: &str) -> String {
    let words: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            '’' => '\'',
            c if c.is_alphabetic() || c == '\'' => c,
            _ => ' ',
        })
        .collect();
    format!(" {} ", words.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// What a build round is graded on: the Rust twin of `norm` in
/// `src/lib/grading.ts` (lowercase, punctuation to spaces, spaces
/// collapsed). The two must stay in step.
fn graded_as(s: &str) -> String {
    let spaced: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The tiles of a build round: the sentence's words out of order. Never in
/// an order that is graded as right before a tile is touched, which is not
/// only the original order: tiles that are the same word may swap, and one
/// that is only punctuation may be anywhere. `None` for a sentence with no
/// wrong order ("Sí, sí.").
fn build_tiles(es: &str) -> Option<Vec<String>> {
    let mut tiles = shuffle(es.split_whitespace().map(String::from).collect::<Vec<_>>());
    let solved = |tiles: &[String]| graded_as(&tiles.join(" ")) == graded_as(es);
    if !solved(&tiles) {
        return Some(tiles);
    }
    // Not shuffled again until it differs: the shuffle is seeded from the
    // clock, and a retry inside the same tick would deal the same order for
    // ever. Swapping two neighbouring words changes what is graded, unless
    // they are the same word.
    let worded: Vec<usize> = (0..tiles.len()).filter(|i| !graded_as(&tiles[*i]).is_empty()).collect();
    for pair in worded.windows(2) {
        tiles.swap(pair[0], pair[1]);
        if !solved(&tiles) {
            return Some(tiles);
        }
        tiles.swap(pair[0], pair[1]);
    }
    None
}

/// Groups of four words for match rounds: at most `chunks` of them, drawn at
/// random from `pool`, each word in one group at most. Two words share a
/// group only if no tile of one could be paired with a tile of the other and
/// be right: they have no meaning in common ("ahí" and "allí" are both
/// "there"), and in Spanish, not the same definition either. A word without
/// a meaning in the round's language is left out, since its tile in the
/// other language would give its pair away.
fn match_chunks(pool: &[WordCard], lang: &str, chunks: usize) -> Vec<Vec<WordCard>> {
    struct Entry {
        word: WordCard,
        lemma: String,
        shown: String,
        en: HashSet<String>,
        es: HashSet<String>,
    }
    let spanish = lang == "es";
    let mut used = HashSet::new();
    let mut open: Vec<Vec<Entry>> = Vec::new();
    let mut full = 0;
    for word in shuffle(pool.to_vec()) {
        if full >= chunks {
            break;
        }
        let meaning = if spanish { &word.gloss_es } else { &word.gloss_en };
        let Some(shown) = meaning.as_ref().map(|m| m.trim().to_lowercase()) else {
            continue;
        };
        if !used.insert(word.word_id) {
            continue;
        }
        let entry = Entry {
            lemma: word.lemma.to_lowercase(),
            shown,
            en: gloss_parts(word.gloss_en.as_deref()),
            es: gloss_parts(word.gloss_es.as_deref()),
            word,
        };
        let apart = |other: &Entry| {
            other.lemma != entry.lemma
                && other.shown != entry.shown
                && other.en.is_disjoint(&entry.en)
                && (!spanish || other.es.is_disjoint(&entry.es))
        };
        match open.iter_mut().find(|chunk| chunk.len() < 4 && chunk.iter().all(apart)) {
            Some(chunk) => {
                chunk.push(entry);
                full += usize::from(chunk.len() == 4);
            }
            None => open.push(vec![entry]),
        }
    }
    open.into_iter()
        .filter(|chunk| chunk.len() == 4)
        .take(chunks)
        .map(|chunk| chunk.into_iter().map(|entry| entry.word).collect())
        .collect()
}

/// (byte offset, word) for each run of letters in `s`.
fn words_in(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices() {
        if c.is_alphabetic() {
            start.get_or_insert(i);
        } else if let Some(st) = start.take() {
            out.push((st, &s[st..i]));
        }
    }
    if let Some(st) = start {
        out.push((st, &s[st..]));
    }
    out
}

fn conjugation_round(ctx: &mut GenCtx, id: i64, w: &WordCard) -> Option<(Round, &'static str)> {
    let rows = ctx.content.conjugations(w.word_id);
    let tenses = ["presente", "pretérito", "imperfecto", "futuro", "subjuntivo presente"];
    let seed = (id as u64).wrapping_mul(2654435761) ^ 0x9E3779B9;
    let tense = tenses[(seed % tenses.len() as u64) as usize];
    let candidates: Vec<&tilde_core::types::ConjRow> = rows
        .iter()
        .filter(|r| r.tense == tense && !r.person.is_empty())
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let row = candidates[(seed / 7 % candidates.len() as u64) as usize];
    let hint = row.form.chars().next().map(|c| {
        format!(
            "{}{}",
            c,
            "_".repeat(row.form.chars().count().saturating_sub(1))
        )
    })?;
    Some((
        Round::Conjugation {
            id,
            word_id: w.word_id,
            verb: w.lemma.clone(),
            tense: tense.to_string(),
            person: row.person.clone(),
            answer: row.form.clone(),
            hint,
        },
        "conjugation",
    ))
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

pub fn placement_items(content: &ContentDb) -> Vec<PlacementItem> {
    let bands = [40i64, 120, 350, 800, 1600, 2800, 4500, 6500, 9000, 11500];
    let mut items = Vec::new();
    for band in bands {
        let Some(w) = content.words_by_rank(band, 1).into_iter().next() else {
            continue;
        };
        // placement is asked in English, whatever the definitions setting
        let (options, answer_index) = options_for(content, &w, Shown::Meaning, "en");
        items.push(PlacementItem {
            word_id: w.word_id,
            lemma: w.lemma,
            options,
            answer_index,
            band: w.level,
        });
    }
    shuffle(items)
}

/// Placement never takes words past this rank as known. Its harder questions
/// are thousands of ranks apart and say little about the words between them;
/// and a word wrongly taken as known is never taught, while one wrongly left
/// costs the player a tap on its intro card.
const PLACEMENT_MAX_CUT: i64 = 1800;

/// The rank below which placement takes every word as known, from the
/// answers as (rank of the word, right or wrong), commonest word first.
///
/// With four options a guess is right one time in four, so the number of
/// right answers says little; where they fall says more. Right answers count
/// up to the second miss: one slip is forgiven, but past two misses a right
/// answer is as likely a guess. Of those, the hardest may be a guess too, so
/// the cut is the second-hardest; and a single right answer is no evidence
/// of anything, so then nothing is marked (a cut of 1).
fn placement_cut(ranked: &[(i64, bool)]) -> i64 {
    let mut kept = Vec::new();
    let mut misses = 0;
    for &(rank, correct) in ranked {
        if correct {
            kept.push(rank);
        } else {
            misses += 1;
            if misses == 2 {
                break;
            }
        }
    }
    if kept.len() < 2 {
        return 1;
    }
    kept[kept.len() - 2].min(PLACEMENT_MAX_CUT)
}

/// Works out the placement and saves it: every teachable word more frequent
/// than the cut becomes a known word. Up to a couple of thousand writes: the
/// caller runs it inside its transaction.
pub fn placement_result(
    user: &Connection,
    content: &ContentDb,
    answers: &[PlacementAnswer],
) -> rusqlite::Result<PlacementResult> {
    // by the word's rank, whatever order or band the questions came in; an
    // id asked twice counts once, one that isn't a word not at all
    let mut asked = HashSet::new();
    let mut ranked: Vec<(i64, bool)> = answers
        .iter()
        .filter(|a| asked.insert(a.word_id))
        .filter_map(|a| content.word(a.word_id).map(|w| (w.rank, a.correct)))
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    let cut = placement_cut(&ranked);

    // A word the player has met already is left alone: its card knows more
    // about it than ten questions do.
    let carded = deck::card_ids(user);
    let mut marked = 0i64;
    for id in content.teachable_below(cut) {
        if !carded.contains(&id) {
            deck::mark_known(user, id)?;
            marked += 1;
        }
    }
    // for the record; new words don't start from it (see `new_candidates`)
    crate::db::set_setting(user, "frontier_rank", &cut.to_string())?;
    let known = deck::counts(user, content.word_ids()).2;
    Ok(PlacementResult {
        frontier_rank: cut,
        words_marked_known: marked,
        cefr_estimate: cefr_estimate(known).to_string(),
    })
}

/// The level shown to the player, from how many words they know. Home and
/// the placement result both use it, so they can't disagree.
pub fn cefr_estimate(words_known: i64) -> &'static str {
    match words_known {
        0..=120 => "A1",
        121..=500 => "A2",
        501..=1200 => "B1",
        1201..=2200 => "B2",
        _ => "C1",
    }
}

// ---------------------------------------------------------------------------
// Feedback / XP
// ---------------------------------------------------------------------------

pub fn level_info(xp: i64) -> LevelInfo {
    let (level, name) = tilde_core::level_for_xp(xp);
    LevelInfo {
        level,
        name: name.to_string(),
        xp_into_level: xp % 500,
        xp_for_next: 500,
    }
}

pub fn correct_answer_of(round: &Round) -> Option<String> {
    match round {
        Round::Choice { options, answer_index, .. } => {
            options.get(*answer_index as usize).cloned()
        }
        Round::Listen { options, answer_index, .. } => options.get(*answer_index as usize).cloned(),
        Round::ListenType { answer, .. } => Some(answer.clone()),
        Round::Build { answer, .. } => Some(answer.clone()),
        Round::Cloze { options, answer_index, .. } => options.get(*answer_index as usize).cloned(),
        Round::Conjugation { answer, .. } => Some(answer.clone()),
        _ => None,
    }
}

pub struct ScoreOutcome {
    pub xp_gained: i32,
    pub combo: i32,
    pub new_badges: Vec<Badge>,
}

/// The level reached, if going from `before` to `after` lifetime XP crossed one.
pub fn level_up(before: i64, after: i64) -> Option<LevelInfo> {
    let reached = level_info(after);
    (reached.level > level_info(before).level).then_some(reached)
}

pub fn score_round(
    user: &Connection,
    round_type: &str,
    correct: bool,
    quality: Option<i32>,
    combo: &mut i32,
    best_combo: &mut i32,
    session_xp: &mut i64,
) -> rusqlite::Result<ScoreOutcome> {
    let mut earned_badges = Vec::new();

    // an intro card is shown, not answered: it neither adds to a run of
    // right answers nor breaks one
    if round_type != "new_word" {
        if correct {
            *combo += 1;
            *best_combo = (*best_combo).max(*combo);
        } else {
            *combo = 0;
        }
    }

    let xp: i32 = if round_type == "review_card" {
        match quality.unwrap_or(3) {
            1 => 2,
            2 => 7,
            3 => 10,
            _ => 12,
        }
    } else if round_type == "match" {
        if correct {
            12
        } else {
            4
        }
    } else if round_type == "new_word" {
        0
    } else if correct {
        10 + (*combo / 5).min(5) * 2
    } else {
        1
    };

    *session_xp += xp as i64;

    // combo badges
    if *combo >= 10 && award_badge(user, "combo-10")? {
        earned_badges.push(find_badge("combo-10"));
    }
    if *combo >= 25 && award_badge(user, "combo-25")? {
        earned_badges.push(find_badge("combo-25"));
    }

    Ok(ScoreOutcome {
        xp_gained: xp,
        combo: *combo,
        new_badges: earned_badges,
    })
}

pub fn find_badge(id: &str) -> Badge {
    tilde_core::all_badges()
        .into_iter()
        .find(|b| b.id == id)
        .unwrap_or(Badge {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            earned_at: None,
        })
}

/// Returns true if the badge was newly earned.
///
/// Having it already is not an error (OR IGNORE), so an error here is a
/// failed write like any other. Inside a transaction it must end the command
/// (`?`): SQLite may have dropped the whole transaction with it, and whatever
/// was written next would be saved on its own.
pub fn award_badge(user: &Connection, id: &str) -> rusqlite::Result<bool> {
    let added = user.execute(
        "INSERT OR IGNORE INTO badges(id, earned_at) VALUES (?1, ?2)",
        rusqlite::params![id, deck::now() as i64],
    )?;
    Ok(added > 0)
}

pub fn session_summary(
    user: &Connection,
    session: &Session,
    level_up: Option<LevelInfo>,
    new_badges: Vec<Badge>,
) -> SessionSummary {
    // the same whole seconds the session's `events` row holds, so this and
    // Home's total count the same time
    let minutes = (session.active_ms / 1000) as f64 / 60.0;
    let accuracy = if session.total_graded > 0 {
        session.correct as f32 / session.total_graded as f32
    } else {
        0.0
    };
    let mut badges = new_badges;
    // no transaction here: a badge that can't be written just isn't earned yet
    if session.total_graded >= 5 && (accuracy - 1.0).abs() < f32::EPSILON && award_badge(user, "perfect-session").unwrap_or(false) {
        badges.push(find_badge("perfect-session"));
    }
    SessionSummary {
        xp: session.xp,
        rounds: session.total_graded,
        correct: session.correct,
        minutes,
        new_words: session.new_words.clone(),
        accuracy,
        level_up,
        new_badges: badges,
        best_combo: session.best_combo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn content() -> ContentDb {
        ContentDb::open(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/content.db"
        )))
        .unwrap()
    }

    /// A directory that is removed again when the test is done with it.
    struct Scratch(std::path::PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A new, empty progress database in a directory of its own. Bind the
    /// directory first, as returned, so that it outlives the connection.
    fn fresh_user() -> (Scratch, Connection) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tilde_session_test_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let user = crate::db::open(&dir.join("tilde_user.db")).unwrap();
        (Scratch(dir), user)
    }

    fn with_ctx<T>(f: impl FnOnce(&mut GenCtx) -> T) -> T {
        with_ctx_in("en", Tts::off, f)
    }

    /// A voice that is there but never asked to speak: generating rounds
    /// only looks in the audio cache.
    fn voice(dir: &std::path::Path) -> Tts {
        Tts::with_espeak(dir, "/bin/true".into())
    }

    fn with_ctx_speaking<T>(tts: impl FnOnce(&std::path::Path) -> Tts, f: impl FnOnce(&mut GenCtx) -> T) -> T {
        with_ctx_in("en", tts, f)
    }

    /// A context with definitions in `lang` ("en", or "es" for Español primero).
    fn with_ctx_in<T>(lang: &str, tts: impl FnOnce(&std::path::Path) -> Tts, f: impl FnOnce(&mut GenCtx) -> T) -> T {
        with_ctx_over(&content(), lang, tts, f)
    }

    /// The same over a dictionary the test chose: the bundled one, or one it made.
    fn with_ctx_over<T>(content: &ContentDb, lang: &str, tts: impl FnOnce(&std::path::Path) -> Tts, f: impl FnOnce(&mut GenCtx) -> T) -> T {
        let (dir, user) = fresh_user();
        let tts = tts(&dir.0);
        let (mut used, mut introduced) = (HashSet::new(), HashSet::new());
        let mut ctx = GenCtx {
            content,
            user: &user,
            tts: &tts,
            definition_lang: lang.into(),
            used_sentences: &mut used,
            introduced: &mut introduced,
            to_speak: Default::default(),
        };
        f(&mut ctx)
    }

    fn word(content: &ContentDb, lemma: &str) -> WordCard {
        let id = content.word_ids_by_lemma(lemma)[0];
        content.word(id).unwrap()
    }

    /// An example sentence as the dictionary hands it over: `form` is the
    /// token it is linked to its word by.
    fn example(es: &str, en: Option<&str>, form: &str) -> Example {
        Example { id: 0, es: es.into(), en: en.map(String::from), form: form.into() }
    }

    #[test]
    fn cloze_blanks_the_inflected_form_with_distractors_in_the_same_form() {
        with_ctx(|ctx| {
            let tener = word(ctx.content, "tener");
            let dog = example("Yo tengo un perro muy grande.", Some("I have a very big dog."), "tengo");
            let (round, _) = cloze_round(ctx, 0, &tener, &dog).unwrap();
            let Round::Cloze { sentence_es, options, answer_index, .. } = round else {
                panic!("not a cloze round")
            };
            assert_eq!(sentence_es, "Yo ___ un perro muy grande.");
            assert_eq!(options[answer_index as usize], "tengo");
            for o in &options {
                let same_form: bool = ctx
                    .content
                    .conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM forms WHERE form = ?1 AND tag = 'presente|yo')",
                        [o],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert!(same_form, "{o} is not a presente/yo form: {options:?}");
            }
            // no gap without the linked token in the sentence, or for a token
            // that is no form of the word
            let nothing = |form| example("No hay nada aquí.", Some("There is nothing here."), form);
            assert!(cloze_round(ctx, 0, &tener, &nothing("tengo")).is_none());
            assert!(cloze_round(ctx, 0, &tener, &nothing("hay")).is_none());
            assert!(cloze_round(ctx, 0, &tener, &nothing("")).is_none());
        });
    }

    #[test]
    fn cloze_blanks_the_linked_token() {
        // As older dictionaries had it: a noun's masculine listed among its
        // forms, so that "padre" is a form of "madre".
        let family = [(1, "madre", "padre", "mother"), (2, "tía", "tío", "aunt"), (3, "hermana", "hermano", "sister"), (4, "abuela", "abuelo", "grandmother")];
        let mut rows = String::new();
        for (id, lemma, masculine, gloss) in family {
            rows += &format!(
                "INSERT INTO words(id, lemma, lemma_key, pos, rank, gloss_en) VALUES ({id}, '{lemma}', '{lemma}', 'noun', {id}, '{gloss}');
                 INSERT INTO forms(form, word_id, tag) VALUES ('{lemma}', {id}, ''), ('{lemma}s', {id}, 'plural'), ('{masculine}', {id}, 'masculine');"
            );
        }
        rows += "INSERT INTO sentences(id, es, en) VALUES
                   (1, 'Mi padre y mi madre.', 'My father and my mother.'),
                   (2, 'Los padres y las madres.', 'The fathers and the mothers.');
                 INSERT INTO sentence_links(sentence_id, word_id, form, quality) VALUES (1, 1, 'madre', 0), (2, 1, 'madres', 0);";
        let content = ContentDb::scratch(&rows);
        with_ctx_over(&content, "en", Tts::off, |ctx| {
            let madre = word(ctx.content, "madre");
            let gaps: Vec<(String, String)> = ctx
                .content
                .sentences_for_word(madre.word_id, 4)
                .iter()
                .map(|example| {
                    let (round, _) = cloze_round(ctx, 0, &madre, example).expect("a cloze for each example");
                    let Round::Cloze { sentence_es, options, answer_index, .. } = round else { panic!("not a cloze round") };
                    (sentence_es, options[answer_index as usize].clone())
                })
                .collect();
            // the gap is the word on the card, not the first token listed with it
            let these = |gaps: [(&str, &str); 2]| gaps.map(|(es, answer)| (es.to_string(), answer.to_string())).to_vec();
            assert_eq!(gaps, these([("Mi padre y mi ___.", "madre"), ("Los padres y las ___.", "madres")]));
        });
    }

    #[test]
    fn conjugation_rounds_come_from_the_wiktionary_table() {
        with_ctx(|ctx| {
            let decir = word(ctx.content, "decir");
            let table = ctx.content.conjugations(decir.word_id);
            for id in 0..20 {
                let (round, _) = conjugation_round(ctx, id, &decir).unwrap();
                let Round::Conjugation { tense, person, answer, .. } = round else { panic!() };
                assert!(table.iter().any(|r| r.tense == tense && r.person == person && r.form == answer));
            }
            let row = |t: &str, p: &str| table.iter().find(|r| r.tense == t && r.person == p).map(|r| r.form.clone());
            assert_eq!(row("subjuntivo presente", "nosotros").as_deref(), Some("digamos"));
        });
    }

    #[test]
    fn only_real_verbs_are_drilled_and_flagged_words_are_not_introduced() {
        let content = content();
        assert!(content.is_verb(word(&content, "tener").word_id));
        for noun in ["mujer", "lugar", "ayer"] {
            assert!(!content.is_verb(word(&content, noun).word_id), "{noun}");
        }
        let flagged: i64 = content
            .conn
            .query_row(
                "SELECT COUNT(*) FROM words WHERE register IS NOT NULL OR region IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(flagged > 0);
        let taught = content.new_candidates(12_000, &HashSet::new());
        // vulgar words, and one that Spain has another word for
        for lemma in ["mierda", "joder", "computadora", "puto", "coño", "follar", "carajo"] {
            // (by rank: a failing test is no place to print these)
            let rank = word(&content, lemma).rank;
            assert!(!taught.iter().chain(&content.verbs(2_000)).any(|w| w.lemma == lemma), "the flagged word of rank {rank} is taught");
        }
        // and two that were once taken for Latin American, and are said in Spain
        for lemma in ["herir", "arrojar"] {
            assert!(taught.iter().any(|w| w.lemma == lemma), "{lemma} is never taught");
        }
    }

    #[test]
    fn distractors_never_mean_the_same_thing() {
        let content = content();
        let lemmas = |w: &str| -> Vec<String> {
            content.similar_words(&word(&content, w), 12).into_iter().map(|d| d.lemma).collect()
        };
        assert!(!lemmas("ser").iter().any(|l| l == "estar" || l == "haber"), "{:?}", lemmas("ser"));
        assert!(!lemmas("tu").iter().any(|l| l == "su"), "{:?}", lemmas("tu"));
        // "they, them" and "to them, for them": the "to" doesn't make it another meaning
        assert!(!lemmas("ellos").iter().any(|l| l == "les"), "{:?}", lemmas("ellos"));

        let parts = |g: &Option<String>| -> HashSet<String> {
            g.as_deref()
                .unwrap_or("")
                .split([';', ','])
                .map(|p| p.trim().to_lowercase())
                .map(|p| p.strip_prefix("to ").unwrap_or(&p).to_string())
                .filter(|p| !p.is_empty())
                .collect()
        };
        for w in content.words_by_rank(1, 300) {
            for d in content.similar_words(&w, 8).iter().chain(&content.distractor_candidates(&w, 40)) {
                assert!(parts(&w.gloss_en).is_disjoint(&parts(&d.gloss_en)), "{} / {}: {:?} vs {:?}", w.lemma, d.lemma, w.gloss_en, d.gloss_en);
            }
        }
    }

    #[test]
    fn wrong_answers_are_words_that_could_be_taught() {
        let content = content();
        let teachable: HashSet<i64> = content.new_candidates(usize::MAX, &HashSet::new()).iter().map(|w| w.word_id).collect();
        // Words that are never taught (used only outside Spain, or vulgar),
        // each next to its nearest neighbour of the same class: where it
        // would be offered first, if it were offered at all.
        let mut left_out: Vec<WordCard> = content
            .word_ids()
            .iter()
            .filter(|id| !teachable.contains(id))
            .filter_map(|id| content.word(*id))
            .filter(|w| w.rank <= 3000)
            .collect();
        left_out.sort_by_key(|w| w.rank);
        assert!(!left_out.is_empty());
        for outsider in &left_out {
            let Some(neighbour) = content.similar_words(outsider, 1).pop() else { continue };
            let same_class = content.similar_words(&neighbour, 12_000);
            let any_class = content.distractor_candidates(&neighbour, 12_000);
            assert!(same_class.len() > 3 && any_class.len() > same_class.len(), "{}", neighbour.lemma);
            for d in same_class.iter().chain(&any_class) {
                assert!(teachable.contains(&d.word_id), "rank {} is offered as a wrong answer for {}", d.rank, neighbour.lemma);
            }
            // own class first, then the rest; nearest in frequency first in each
            let own: Vec<bool> = any_class.iter().map(|d| d.pos == neighbour.pos).collect();
            assert!(own.windows(2).all(|p| p[0] || !p[1]), "{}: a word of another class before one of its own", neighbour.lemma);
            let distance = |d: &WordCard| (d.rank - neighbour.rank).abs();
            assert!(any_class.windows(2).all(|p| (p[0].pos == neighbour.pos) != (p[1].pos == neighbour.pos) || distance(&p[0]) <= distance(&p[1])));
        }
    }

    fn options_of(round: &Round) -> (&[String], usize) {
        match round {
            Round::Choice { options, answer_index, .. }
            | Round::Listen { options, answer_index, .. }
            | Round::Cloze { options, answer_index, .. } => (options, *answer_index as usize),
            _ => panic!("a round without options"),
        }
    }

    fn assert_four_that_read_differently(options: &[String], what: &str) {
        assert_eq!(options.len(), 4, "{what}: {options:?}");
        let read: HashSet<String> = options.iter().map(|o| o.trim().to_lowercase()).collect();
        assert_eq!(read.len(), 4, "{what}: two options read the same: {options:?}");
    }

    /// Every teachable word, by lemma.
    fn dictionary(content: &ContentDb) -> HashMap<String, WordCard> {
        content.new_candidates(usize::MAX, &HashSet::new()).into_iter().map(|w| (w.lemma.clone(), w)).collect()
    }

    #[test]
    fn every_multiple_choice_round_has_four_distinct_options() {
        for lang in ["en", "es"] {
            with_ctx_in(lang, voice, |ctx| {
                let words = dictionary(ctx.content);
                // the commonest words: the articles, "sí", "del" and "al" are
                // among them, which have few or no words of their own class
                for w in ctx.content.words_by_rank(1, 600) {
                    let what = format!("{} ({lang})", w.lemma);
                    let meanings = gloss_parts(w.gloss_en.as_deref());

                    let (round, _) = choice_round(ctx, 0, &w);
                    let (options, answer) = options_of(&round);
                    assert_four_that_read_differently(options, &what);
                    assert_eq!(options[answer], gloss_of(&w, lang), "{what}");

                    let (round, _) = choice_round(ctx, 1, &w);
                    let (options, answer) = options_of(&round);
                    assert_four_that_read_differently(options, &what);
                    assert_eq!(options[answer], w.lemma, "{what}");
                    // real words that mean something else, not padding
                    for other in options.iter().filter(|o| **o != w.lemma) {
                        let d = words.get(other).unwrap_or_else(|| panic!("{what}: {other} is no word that is taught"));
                        assert!(gloss_parts(d.gloss_en.as_deref()).is_disjoint(&meanings), "{what}: {other} means the same");
                    }

                    let (round, ty) = listen_round(ctx, 2, &w);
                    assert_eq!(ty, "listen");
                    let (options, answer) = options_of(&round);
                    assert_four_that_read_differently(options, &what);
                    assert_eq!(options[answer], w.lemma, "{what}");
                }
            });
        }
    }

    #[test]
    fn listening_options_never_sound_alike() {
        for (a, b) in [("el", "él"), ("si", "sí"), ("vaca", "baca"), ("hola", "ola"), ("valla", "vaya"), ("hecho", "echo"), ("Tuvo", "tubo")] {
            assert_eq!(sound_key(a), sound_key(b), "{a} / {b}");
        }
        for (a, b) in [("una", "uña"), ("coche", "coce"), ("pero", "perro"), ("casa", "caza")] {
            assert_ne!(sound_key(a), sound_key(b), "{a} / {b}");
        }

        with_ctx_speaking(voice, |ctx| {
            // words that sound like another word, as the ear has it
            let mut alike: HashMap<String, Vec<WordCard>> = HashMap::new();
            for w in dictionary(ctx.content).into_values() {
                alike.entry(sound_key(&w.lemma)).or_default().push(w);
            }
            let mut with_a_twin = 0;
            for w in ctx.content.words_by_rank(1, 600) {
                let (round, _) = listen_round(ctx, 0, &w);
                let (options, answer) = options_of(&round);
                assert_four_that_read_differently(options, &w.lemma);
                assert_eq!(options[answer], w.lemma);
                let sounds: HashSet<String> = options.iter().map(|o| sound_key(o)).collect();
                assert_eq!(sounds.len(), 4, "{}: two options sound the same: {options:?}", w.lemma);

                // sidecar says the word and shows meanings: the meaning of a
                // word that sounds the same is as right as the answer
                let (meanings, _) = options_for(ctx.content, &w, Shown::HeardMeaning, "en");
                assert_four_that_read_differently(&meanings, &w.lemma);
                for twin in alike[&sound_key(&w.lemma)].iter().filter(|t| t.gloss_en != w.gloss_en) {
                    with_a_twin += 1;
                    assert!(!meanings.contains(&gloss_in(twin, "en")), "{}: offers the meaning of {}", w.lemma, twin.lemma);
                }
            }
            assert!(with_a_twin > 0, "no common word sounds like another: nothing was tested");

            // Words short of others of their own class, which get wrong
            // answers from other classes: their twins among them.
            let mut would_be_offered = 0;
            for (heard, twin) in [("sí", "si"), ("el", "él")] {
                let w = word(ctx.content, heard);
                let nearest = ctx.content.distractor_candidates(&w, 3);
                would_be_offered += usize::from(nearest.iter().any(|d| d.lemma == twin));
                for _ in 0..20 {
                    let (round, _) = listen_round(ctx, 0, &w);
                    let (options, answer) = options_of(&round);
                    assert_eq!((options.len(), options[answer].as_str()), (4, heard));
                    assert!(!options.iter().any(|o| o == twin), "{heard}: {options:?}");
                }
            }
            assert!(would_be_offered > 0, "neither twin is among the nearest words any more: pick another pair");
        });
    }

    #[test]
    fn spanish_first_meaning_rounds_use_one_language() {
        let content = content();
        let glosses = |column: &str| -> HashSet<String> {
            let mut stmt = content.conn.prepare(&format!("SELECT {column} FROM words WHERE {column} IS NOT NULL")).unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        let (spanish, english) = (glosses("gloss_es"), glosses("gloss_en"));
        let mut common = content.words_by_rank(1, 600);
        // Nearly all of those have a Spanish definition; the commonest words
        // that have none are further down, so both kinds are looked at.
        let seen: HashSet<i64> = common.iter().map(|w| w.word_id).collect();
        let undefined = content.new_candidates(usize::MAX, &seen).into_iter().filter(|w| w.gloss_es.is_none());
        common.extend(undefined.take(40));
        let defined = common.iter().filter(|w| w.gloss_es.is_some()).count();
        assert!(defined >= 300, "only {defined} of the words looked at have a Spanish definition");
        assert!(common.len() - defined >= 40, "only {} of the words looked at have no Spanish definition", common.len() - defined);

        with_ctx_in("es", voice, |ctx| {
            let words = dictionary(ctx.content);
            for w in &common {
                let definition = gloss_parts(w.gloss_es.as_deref());
                let (round, _) = choice_round(ctx, 0, w);
                let shown = [
                    options_of(&round).0.to_vec(),
                    // as sidecar builds them
                    options_for(ctx.content, w, Shown::Meaning, "es").0,
                    options_for(ctx.content, w, Shown::HeardMeaning, "es").0,
                ];
                for options in &shown {
                    assert_eq!(options.len(), 4, "{}", w.lemma);
                    for o in options {
                        if let Some(own) = &w.gloss_es {
                            assert!(spanish.contains(o), "{}: \"{o}\" among Spanish definitions", w.lemma);
                            assert!(o == own || gloss_parts(Some(o)).is_disjoint(&definition), "{}: \"{o}\" defines it too", w.lemma);
                        } else {
                            assert!(english.contains(o), "{}: \"{o}\" among English meanings", w.lemma);
                        }
                    }
                }

                // from a Spanish definition to the word: only one word it defines
                let (round, _) = choice_round(ctx, 1, w);
                let Round::Choice { prompt, options, .. } = &round else { panic!() };
                assert_eq!(*prompt, gloss_of(w, "es"));
                if w.gloss_es.is_some() {
                    for other in options.iter().filter(|o| **o != w.lemma) {
                        assert!(gloss_parts(words[other].gloss_es.as_deref()).is_disjoint(&definition), "{}: \"{prompt}\" defines {other} too", w.lemma);
                    }
                }
            }

        });
    }

    /// A word nobody has to look up in the dictionary to test a rule with.
    fn made_up(id: i64, lemma: &str, gloss_en: &str, gloss_es: Option<&str>) -> WordCard {
        WordCard {
            word_id: id,
            lemma: lemma.into(),
            pos: Some("noun".into()),
            rank: id,
            gloss_en: Some(gloss_en.into()),
            gloss_es: gloss_es.map(String::from),
            level: "A1".into(),
        }
    }

    #[test]
    fn an_option_that_would_be_right_too_is_left_out() {
        let vaca = made_up(1, "vaca", "cow", Some("Hembra del toro"));
        let candidates = vec![
            // sounds like the word asked about
            made_up(2, "baca", "roof rack", Some("Soporte que se coloca en el techo")),
            // is defined partly as it is
            made_up(3, "res", "head of cattle", Some("Animal vacuno, hembra del toro")),
            // reads like it, and its meaning like the one before last
            made_up(4, "Vaca", " Roof rack ", None),
            made_up(5, "toro", "bull", Some("Macho de la vaca")),
            made_up(6, "valla", "fence", Some("Cerca")),
            // sounds like the one before
            made_up(7, "vaya", "wow", None),
            made_up(8, "cerdo", "pig", Some("Mamífero doméstico")),
            made_up(9, "pato", "duck", Some("Ave acuática")),
        ];
        // (the right option, the wrong ones in alphabetical order)
        let offered = |w: &WordCard, from: &[WordCard], shown: Shown, lang: &str| {
            let (options, answer) = options_from(w, from.to_vec(), shown, lang);
            let right = options[answer as usize].clone();
            let mut wrong: Vec<String> = options.into_iter().filter(|o| *o != right).collect();
            wrong.sort();
            (right, wrong)
        };
        let all = |shown: Shown, lang: &str| offered(&vaca, &candidates, shown, lang);
        let these = |right: &str, wrong: [&str; 3]| (right.to_string(), wrong.map(String::from).to_vec());

        // the first three, but no two that read the same on the buttons
        assert_eq!(all(Shown::Meaning, "en"), these("cow", ["bull", "head of cattle", "roof rack"]));
        assert_eq!(all(Shown::Word, "en"), these("vaca", ["baca", "res", "toro"]));
        // only heard: nothing that sounds like the word, whatever is shown...
        assert_eq!(all(Shown::Heard, "en"), these("vaca", ["res", "toro", "valla"]));
        assert_eq!(all(Shown::Heard, "es"), these("vaca", ["res", "toro", "valla"]));
        assert_eq!(all(Shown::HeardMeaning, "en"), these("cow", ["bull", "fence", "head of cattle"]));
        // ...and no two words that sound like each other
        assert_eq!(offered(&vaca, &candidates[4..], Shown::Heard, "en"), these("vaca", ["cerdo", "pato", "valla"]));
        // Spanish definitions: only from words that have one, and none that
        // also defines the word asked about
        assert_eq!(
            all(Shown::Meaning, "es"),
            these("Hembra del toro", ["Cerca", "Macho de la vaca", "Soporte que se coloca en el techo"])
        );
        assert_eq!(all(Shown::HeardMeaning, "es"), these("Hembra del toro", ["Cerca", "Macho de la vaca", "Mamífero doméstico"]));
        // words to pick for a Spanish definition: any that it doesn't define
        assert_eq!(all(Shown::Word, "es"), these("vaca", ["baca", "toro", "valla"]));

        // a word with no Spanish definition is asked about in English throughout
        let vaya = &candidates[5];
        assert_eq!(offered(vaya, &candidates[..5], Shown::Meaning, "es"), these("wow", ["bull", "head of cattle", "roof rack"]));

        // with too few words to offer, the round has what there is
        let (options, answer) = options_from(&vaca, candidates[..2].to_vec(), Shown::Meaning, "en");
        assert_eq!((options.len(), options[answer as usize].as_str()), (3, "cow"));
        assert_eq!(options_from(&vaca, vec![], Shown::Heard, "en"), (vec!["vaca".to_string()], 0));
    }

    /// Two common words whose English glosses have nothing in common, but
    /// whose Spanish definitions share a part.
    fn defined_alike(content: &ContentDb) -> (WordCard, WordCard) {
        let common = content.words_by_rank(1, 1500);
        let en = |w: &WordCard| gloss_parts(w.gloss_en.as_deref());
        let es = |w: &WordCard| gloss_parts(w.gloss_es.as_deref());
        for (i, b) in common.iter().enumerate() {
            if let Some(a) = common[..i].iter().find(|a| en(a).is_disjoint(&en(b)) && !es(a).is_disjoint(&es(b))) {
                return (a.clone(), b.clone());
            }
        }
        panic!("no two common words share a Spanish definition only");
    }

    #[test]
    fn match_chunks_never_hold_two_words_that_mean_the_same() {
        let content = content();
        let mut pool: Vec<WordCard> =
            ["ahí", "allí", "aquí", "acá", "eso", "ese", "esto", "este", "casa", "perro", "comer", "grande", "ayer", "libro"]
                .iter()
                .map(|lemma| word(&content, lemma))
                .collect();
        let (one, other) = defined_alike(&content);
        pool.extend([one, other]);
        // the commonest word that has no Spanish definition
        pool.extend(content.new_candidates(usize::MAX, &HashSet::new()).into_iter().find(|w| w.gloss_es.is_none()));
        let en = |w: &WordCard| gloss_parts(w.gloss_en.as_deref());
        let es = |w: &WordCard| gloss_parts(w.gloss_es.as_deref());
        // What the pool is made of: words that share an English meaning, two
        // that share only their Spanish definition, and one that has none.
        let pairs = || pool.iter().enumerate().flat_map(|(i, a)| pool[i + 1..].iter().map(move |b| (a, b)));
        assert!(pairs().filter(|(a, b)| !en(a).is_disjoint(&en(b))).count() >= 4, "the pool's words no longer mean the same");
        assert!(pool.iter().any(|w| w.gloss_es.is_none()), "every word of the pool has a Spanish definition now");

        for lang in ["en", "es"] {
            for _ in 0..200 {
                let chunks = match_chunks(&pool, lang, 2);
                assert!(!chunks.is_empty() && chunks.len() <= 2, "{lang}: {} chunks", chunks.len());
                let mut dealt = HashSet::new();
                for chunk in &chunks {
                    assert_eq!(chunk.len(), 4);
                    for (i, a) in chunk.iter().enumerate() {
                        assert!(dealt.insert(a.word_id), "{} is in two rounds", a.lemma);
                        assert!(lang == "en" || a.gloss_es.is_some(), "{} has no Spanish definition to show", a.lemma);
                        for b in &chunk[i + 1..] {
                            assert_ne!(a.lemma, b.lemma);
                            assert!(en(a).is_disjoint(&en(b)), "{lang}: {} and {} mean the same", a.lemma, b.lemma);
                            assert!(lang == "en" || es(a).is_disjoint(&es(b)), "{} and {} are defined the same", a.lemma, b.lemma);
                        }
                    }
                }
            }
        }

        // too few words that can share a round make none, not a bad one
        let alike: Vec<WordCard> = pool[..6].to_vec();
        assert!(match_chunks(&alike, "en", 2).is_empty());
        assert!(match_chunks(&pool, "en", 0).is_empty());
        assert!(match_chunks(&[], "en", 2).is_empty());
        // and a word given twice is still one word
        let twice: Vec<WordCard> = pool[8..12].iter().chain(&pool[8..12]).cloned().collect();
        assert_eq!(match_chunks(&twice, "en", 2).len(), 1);
    }

    #[test]
    fn build_tiles_are_never_dealt_solved() {
        // as `norm` in grading.ts: what the round is graded on
        assert_eq!(graded_as("  ¡Hola — ¿Qué   tal, tío?"), "hola qué tal tío");

        // duplicate words and a tile that is only punctuation: orders other
        // than the original that are graded as right
        for sentence in ["Tengo un perro.", "Hola — ¿qué tal?", "No, no lo sé.", "Te quiero.", "Sí, claro, sí."] {
            let mut words: Vec<&str> = sentence.split_whitespace().collect();
            words.sort();
            for _ in 0..300 {
                let tiles = build_tiles(sentence).unwrap();
                assert_ne!(graded_as(&tiles.join(" ")), graded_as(sentence), "{sentence}: dealt as {tiles:?}");
                let mut dealt: Vec<&str> = tiles.iter().map(String::as_str).collect();
                dealt.sort();
                assert_eq!(dealt, words);
            }
        }
        // no order of these is wrong
        for sentence in ["Sí, sí.", "¡Hola!", "No... no, ¡no!", ""] {
            assert!(build_tiles(sentence).is_none(), "{sentence}");
        }
    }

    /// The options of a cloze round for `w` in a sentence that has it as
    /// `form`, and the right one.
    fn cloze_options(ctx: &mut GenCtx, w: &WordCard, es: &str, en: &str, form: &str) -> Option<(Vec<String>, String)> {
        cloze_round(ctx, 0, w, &example(es, Some(en), form)).map(|(round, _)| {
            let (options, answer) = options_of(&round);
            (options.to_vec(), options[answer].clone())
        })
    }

    #[test]
    fn cloze_needs_a_translation_and_skips_distractors_it_names() {
        with_ctx(|ctx| {
            let tener = word(ctx.content, "tener");
            let es = "Yo tengo un perro muy grande.";
            assert!(cloze_round(ctx, 0, &tener, &example(es, None, "tengo")).is_none());
            assert!(cloze_round(ctx, 0, &tener, &example(es, Some("  "), "tengo")).is_none());
            let Some((Round::Cloze { sentence_en, .. }, _)) = cloze_round(ctx, 0, &tener, &example(es, Some("I have a very big dog."), "tengo")) else {
                panic!("no cloze with a translation")
            };
            assert_eq!(sentence_en, "I have a very big dog.");

            // "completamente" would fit as well, and the translation says so
            let totalmente = word(ctx.content, "totalmente");
            let es = "Estaba totalmente agotado.";
            let (unnamed, answer) = cloze_options(ctx, &totalmente, es, "He was utterly worn out.", "totalmente").unwrap();
            assert_eq!(answer, "totalmente");
            assert!(unnamed.iter().any(|o| o == "completamente"), "completamente is no distractor of totalmente any more: pick another pair");
            let named = cloze_options(ctx, &totalmente, es, "He was completely worn out.", "totalmente");
            assert!(!named.unwrap().0.iter().any(|o| o == "completamente"));

            // the same for whichever words are offered: name one, and it goes
            for lemma in ["totalmente", "casa", "grande", "comer"] {
                let w = word(ctx.content, lemma);
                let es = format!("Dicen que {lemma} es la palabra.");
                let (options, _) = cloze_options(ctx, &w, &es, "Zzz.", lemma).unwrap();
                for d in options.iter().filter(|o| **o != lemma) {
                    let meaning = gloss_parts(word(ctx.content, d).gloss_en.as_deref())
                        .into_iter()
                        .find(|m| !m.contains('('))
                        .unwrap();
                    // as a whole word or phrase, whatever is around it...
                    let en = format!("They say \"{}\"; is that the word?", meaning.to_uppercase());
                    let left = cloze_options(ctx, &w, &es, &en, lemma).map(|(options, _)| options).unwrap_or_default();
                    assert!(!left.contains(d), "{lemma}: {d} is offered though the translation says \"{meaning}\"");
                    // ...but not as a piece of another word
                    let en = format!("They say x{meaning}x.");
                    assert!(cloze_options(ctx, &w, &es, &en, lemma).unwrap().0.contains(d), "{lemma}: {d}");
                }
            }

            // a translation that settles it already costs no options
            let me = word(ctx.content, "me");
            let (options, answer) = cloze_options(ctx, &me, "No me sorprende.", "It doesn't surprise me.", "me").unwrap();
            assert_eq!(answer, "me");
            assert!(options.len() >= 3);
        });
    }

    #[test]
    fn cloze_never_blanks_an_untagged_inflected_form() {
        with_ctx(|ctx| {
            let dar = word(ctx.content, "dar");
            assert!(cloze_options(ctx, &dar, "Dame tu dinero.", "Give me your money.", "dame").is_none());
            // the word itself is still a gap, with other words as they are
            let (options, answer) = cloze_options(ctx, &dar, "Te quiero dar un regalo.", "I want to give you a present.", "dar").unwrap();
            assert_eq!(answer, "dar");
            for o in &options {
                assert!(!ctx.content.word_ids_by_lemma(o).is_empty(), "{o} is not a dictionary form: {options:?}");
            }

            // any spelling listed with a common word without saying which of
            // its forms it is
            let listed: Vec<(String, String)> = {
                let mut stmt = ctx
                    .content
                    .conn
                    .prepare(
                        "SELECT f.form, w.lemma FROM forms f JOIN words w ON w.id = f.word_id
                         WHERE f.form != w.lemma AND w.rank <= 300 AND w.register IS NULL
                         GROUP BY f.form, f.word_id HAVING MAX(f.tag != '') = 0
                         ORDER BY w.rank, f.form",
                    )
                    .unwrap();
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
                rows.map(|r| r.unwrap()).collect()
            };
            assert!(listed.len() > 100, "{} such forms", listed.len());
            for (form, lemma) in &listed {
                let w = word(ctx.content, lemma);
                let es = format!("{form} zzz zzz zzz.");
                assert!(cloze_options(ctx, &w, &es, "Zzz zzz zzz.", form).is_none(), "\"{form}\" was blanked for {lemma}");
            }
        });
    }

    #[test]
    fn placement_items_all_have_four_options() {
        let content = content();
        for _ in 0..5 {
            let items = placement_items(&content);
            assert_eq!(items.len(), 10);
            for item in &items {
                assert_four_that_read_differently(&item.options, &item.lemma);
                let w = content.word(item.word_id).unwrap();
                assert_eq!(item.options.get(item.answer_index as usize), w.gloss_en.as_ref(), "{}", item.lemma);
            }
        }
    }

    #[test]
    fn generated_sessions_keep_what_each_round_promises() {
        let content = content();
        let (dir, user) = fresh_user();
        let tts = voice(&dir.0);
        // a deck with words that are easily taken for one another
        for lemma in ["ahí", "allí", "aquí", "acá", "eso", "ese", "esto", "este", "empezar", "comenzar", "sí", "si", "el", "él", "la", "los", "del", "al"] {
            deck::introduce(&user, word(&content, lemma).word_id).unwrap();
        }
        let parts = |word_id: i64, lang: &str| {
            let w = content.word(word_id).unwrap();
            gloss_parts(if lang == "es" { w.gloss_es.as_deref() } else { w.gloss_en.as_deref() })
        };
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for lang in ["en", "es"] {
            for kind in [SessionKind::Standard, SessionKind::Deep, SessionKind::Deep, SessionKind::Sidecar] {
                let g = generate(kind, &content, &user, &tts, lang);
                for (round, ty) in g.rounds.iter().zip(&g.round_types) {
                    *seen.entry(ty).or_default() += 1;
                    match round {
                        Round::Choice { options, .. } | Round::Listen { options, .. } => {
                            assert_four_that_read_differently(options, ty);
                        }
                        Round::Cloze { options, sentence_en, .. } => {
                            assert!(options.len() >= 3 && !sentence_en.trim().is_empty());
                        }
                        Round::Build { tiles, answer, .. } => {
                            assert_ne!(graded_as(&tiles.join(" ")), graded_as(answer), "dealt solved: {tiles:?}");
                        }
                        Round::Match { pairs, .. } => {
                            assert_eq!(pairs.len(), 4);
                            for (i, a) in pairs.iter().enumerate() {
                                let w = content.word(a.word_id).unwrap();
                                assert_eq!(Some(&a.en), if lang == "es" { w.gloss_es.as_ref() } else { w.gloss_en.as_ref() }, "{}", a.es);
                                for b in &pairs[i + 1..] {
                                    assert!(parts(a.word_id, "en").is_disjoint(&parts(b.word_id, "en")), "{lang}: {} / {}", a.es, b.es);
                                    assert!(lang == "en" || parts(a.word_id, "es").is_disjoint(&parts(b.word_id, "es")), "{} / {}", a.es, b.es);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for ty in ["choice", "listen", "build", "match"] {
            assert!(seen.get(ty).copied().unwrap_or(0) > 0, "no {ty} round in eight sessions: {seen:?}");
        }
    }

    #[test]
    fn a_session_tops_up_its_new_words_while_the_deck_cannot_fill_its_games() {
        let planned = |learning| {
            [SessionKind::Quick, SessionKind::Standard, SessionKind::Deep, SessionKind::ReviewOnly, SessionKind::Sidecar]
                .map(|kind| planned_new_words(kind, learning))
        };
        // twice the usual number at most, however empty the deck
        assert_eq!(planned(0), [4, 6, 12, 0, 0]);
        // then one fewer for each word the deck can lend to the games
        assert_eq!(planned(1), [4, 6, 12, 0, 0]);
        assert_eq!(planned(2), [3, 6, 12, 0, 0]);
        assert_eq!(planned(4), [2, 4, 11, 0, 0]);
        assert_eq!(planned(9), [2, 3, 6, 0, 0]);
        assert_eq!(planned(5_000), [2, 3, 6, 0, 0]);
    }

    #[test]
    fn placement_counts_right_answers_up_to_the_second_miss() {
        let ranks = [40, 120, 350, 800, 1600, 2800, 4500, 6500, 9000, 11500];
        let cut = |right: &[usize]| {
            let answers: Vec<(i64, bool)> = ranks.iter().enumerate().map(|(i, rank)| (*rank, right.contains(&i))).collect();
            placement_cut(&answers)
        };
        // no evidence: nothing, one right answer, or one right answer among guesses
        assert_eq!(placement_cut(&[]), 1);
        assert_eq!(cut(&[]), 1);
        assert_eq!(cut(&[0]), 1);
        assert_eq!(cut(&[5]), 1);
        assert_eq!(cut(&[2, 9]), 1, "a right answer after two misses is not counted");
        // the cut is the second-hardest answer kept
        assert_eq!(cut(&[0, 1]), 40);
        assert_eq!(cut(&[0, 1, 2]), 120);
        assert_eq!(cut(&[0, 1, 2, 3, 4]), 800);
        // one slip is forgiven, wherever it falls
        assert_eq!(cut(&[1, 2, 3]), 350);
        assert_eq!(cut(&[0, 1, 2, 4]), 350);
        assert_eq!(cut(&[0, 1, 2, 4, 6, 7, 8, 9]), 350, "answers past the second miss changed the cut");
        // and however well it goes, the hard questions vouch for no more than this
        assert_eq!(cut(&[0, 1, 2, 3, 4, 5]), 1600);
        assert_eq!(cut(&[0, 1, 2, 3, 4, 5, 6]), PLACEMENT_MAX_CUT);
        assert_eq!(cut(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), PLACEMENT_MAX_CUT);
    }

    fn dictations(g: &Generated) -> Vec<(i64, &str)> {
        g.rounds
            .iter()
            .filter_map(|r| match r {
                Round::ListenType { word_id, sentence_es, answer, .. } => {
                    assert_eq!(sentence_es, answer);
                    Some((*word_id, sentence_es.as_str()))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dictation_only_with_speech_and_for_words_met_before() {
        let content = content();
        let (dir, user) = fresh_user();
        let speaking = voice(&dir.0);
        let silent = Tts::off(&dir.0);
        let count = |g: &Generated, ty: &str| g.round_types.iter().filter(|t| **t == ty).count();

        // A first session: every word in it was first shown minutes ago. Those
        // are listened for one at a time, never as a sentence to write down.
        let first = generate(SessionKind::Standard, &content, &user, &speaking, "en");
        assert_eq!((count(&first, "listen_type"), dictations(&first).len()), (0, 0));
        assert!(count(&first, "listen") > 0);

        let deck: Vec<WordCard> = content.new_candidates(20, &HashSet::new());
        for w in &deck {
            deck::introduce(&user, w.word_id).unwrap();
        }
        // Generates a session and returns how many dictations it holds, having
        // checked each.
        let dictated = |kind: SessionKind| {
            let g = generate(kind, &content, &user, &speaking, "en");
            let found = dictations(&g);
            for (word_id, sentence) in &found {
                assert!(deck.iter().any(|w| w.word_id == *word_id), "{kind:?}: a dictation for a word first shown in this session");
                assert!(sentence.chars().count() <= DICTATION_MAX_CHARS, "{kind:?}: {sentence}");
            }
            assert_eq!(found.len(), count(&g, "listen_type"));
            // one or two in a mixed session, however long
            if kind != SessionKind::Sidecar {
                assert!(found.len() <= (shape(kind).3 / 6).max(1), "{kind:?}: {} dictations", found.len());
            }
            found.len()
        };
        // Which words get which game is drawn at random, so one session may
        // have none, and it takes a few to see the most there can be.
        for (kind, sessions) in [(SessionKind::Standard, 6), (SessionKind::Deep, 2), (SessionKind::Sidecar, 2)] {
            let total: usize = (0..sessions).map(|_| dictated(kind)).sum();
            assert!(total > 0, "{kind:?}: no dictation in {sessions} sessions");
        }
        assert_eq!(dictated(SessionKind::ReviewOnly), 0);

        // with no voice there is nothing to listen to, in any kind of session
        for kind in [SessionKind::Quick, SessionKind::Standard, SessionKind::Deep, SessionKind::Sidecar, SessionKind::ReviewOnly] {
            let voiced = generate(kind, &content, &user, &speaking, "en");
            let g = generate(kind, &content, &user, &silent, "en");
            assert_eq!(g.rounds.len(), voiced.rounds.len(), "{kind:?}: rounds were dropped, not swapped");
            assert_eq!((count(&g, "listen"), count(&g, "listen_type")), (0, 0), "{kind:?} without a speech engine");
            assert!(!g.rounds.iter().any(|r| matches!(r, Round::Listen { .. } | Round::ListenType { .. })));
            assert!(g.to_speak.is_empty());
        }
    }

    #[test]
    fn a_dictation_is_a_sentence_short_enough_to_write_down() {
        let too_long = |es: &str| es.chars().count() > DICTATION_MAX_CHARS;
        // words whose first example is too long, with a shorter one further on
        let content = content();
        let awkward: Vec<WordCard> = content
            .new_candidates(usize::MAX, &HashSet::new())
            .into_iter()
            .filter(|w| {
                let examples = content.sentences_for_word(w.word_id, 4);
                examples.first().is_some_and(|s| too_long(&s.es)) && examples.iter().any(|s| !too_long(&s.es))
            })
            .take(5)
            .collect();
        assert!(!awkward.is_empty(), "no word in the dictionary has a long first example and a short later one");

        with_ctx_speaking(voice, |ctx| {
            // one session: a sentence that is an example of two of the words
            // is dictated for the first of them only
            let mut dictated = HashSet::new();
            for w in &awkward {
                let examples = ctx.content.sentences_for_word(w.word_id, 4);
                let short: Vec<&Example> = examples.iter().filter(|s| !too_long(&s.es) && !dictated.contains(&s.id)).collect();
                // each short example once, in order, and then no more
                for expected in &short {
                    let Some((Round::ListenType { word_id, sentence_es, answer, .. }, "listen_type")) = dictation_round(ctx, 0, w) else {
                        panic!("no dictation for a word with a short example")
                    };
                    assert_eq!((word_id, &sentence_es, &answer), (w.word_id, &expected.es, &expected.es));
                    dictated.insert(expected.id);
                }
                assert!(dictation_round(ctx, 0, w).is_none(), "an example was dictated twice, or a long one was");
            }
            assert!(!dictated.is_empty());
        });
        // and never without a voice to say it
        with_ctx(|ctx| assert!(dictation_round(ctx, 0, &awkward[0]).is_none()));
    }
}
