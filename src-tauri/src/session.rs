//! Session generation, scoring and gamification.

use crate::content::ContentDb;
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

fn gloss_of(w: &WordCard, definition_lang: &str) -> String {
    if definition_lang == "es" {
        w.gloss_es
            .clone()
            .or_else(|| w.gloss_en.clone())
            .unwrap_or_else(|| w.lemma.clone())
    } else {
        w.gloss_en.clone().unwrap_or_else(|| w.lemma.clone())
    }
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
                example_es: sentences.first().map(|(_, es, _)| es.clone()),
                example_en: sentences.first().and_then(|(_, _, en)| en.clone()),
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
        let gloss = gloss_of(w, &ctx.definition_lang);
        let distractors = ctx.content.similar_words(w, 8);
        let mut options = vec![gloss.clone()];
        for d in distractors.iter().take(3) {
            options.push(gloss_of(d, &ctx.definition_lang));
        }
        let opts = shuffle(options);
        let answer_index = opts.iter().position(|o| *o == gloss).unwrap_or(0) as i32;
        // a round that shows no word needs a voice to say it
        if i % 3 == 2 && ctx.tts.available() {
            rounds.push(Round::Listen {
                id,
                word_id: w.word_id,
                es: w.lemma.clone(),
                audio_base64: ctx.audio(&w.lemma),
                options: opts,
                answer_index,
            });
            types.push("listen");
        } else {
            rounds.push(Round::Choice {
                id,
                word_id: w.word_id,
                prompt: w.lemma.clone(),
                prompt_lang: "es".into(),
                options: opts,
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
                example_es: sentences.first().map(|(_, es, _)| es.clone()),
                example_en: sentences.first().and_then(|(_, _, en)| en.clone()),
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
        let usable: Vec<&(i64, String, Option<String>)> = sentences
            .iter()
            .filter(|(sid, _, _)| !ctx.used_sentences.contains(sid))
            .collect();

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
                if let Some((sid, es, en)) = usable.first() {
                    ctx.used_sentences.insert(*sid);
                    rounds.push(Round::Build {
                        id: rid,
                        word_id: w.word_id,
                        sentence_es: es.clone(),
                        sentence_en: en.clone().unwrap_or_default(),
                        tiles: shuffle(
                            es.split_whitespace()
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>(),
                        ),
                        answer: es.clone(),
                    });
                    types.push("build");
                } else {
                    let (round, ty) = choice_round(ctx, rid, &w);
                    rounds.push(round);
                    types.push(ty);
                }
            }
            _ => {
                let cloze = usable.iter().find_map(|(sid, es, en)| {
                    cloze_round(ctx, rid, &w, es, en.as_deref()).map(|r| (*sid, r))
                });
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
    if game_pool.len() >= 4 {
        for chunk in pick(&game_pool, (n_games / 6).min(2) * 4).chunks(4) {
            if chunk.len() == 4 {
                rounds.push(Round::Match {
                    id: rid,
                    pairs: chunk
                        .iter()
                        .map(|w| MatchPair {
                            word_id: w.word_id,
                            es: w.lemma.clone(),
                            en: gloss_of(w, &ctx.definition_lang),
                        })
                        .collect(),
                });
                types.push("match");
                rid += 1;
            }
        }
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
                example_es: sentences.first().map(|(_, es, _)| es.clone()),
                example_en: sentences.first().and_then(|(_, _, en)| en.clone()),
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
    let en_to_es = id % 2 == 1;
    let gloss = gloss_of(w, &ctx.definition_lang);
    let _ = &gloss;
    let distractors = ctx.content.similar_words(w, 8);
    let mut options = vec![gloss.clone()];
    for d in distractors.iter().take(3) {
        options.push(gloss_of(d, &ctx.definition_lang));
    }
    let opts = shuffle(options);
    let answer_index = opts.iter().position(|o| *o == gloss).unwrap_or(0) as i32;
    if en_to_es {
        let opts2 = shuffle(vec![
            w.lemma.clone(),
            distractors.first().map(|d| d.lemma.clone()).unwrap_or_else(|| "comer".into()),
            distractors.get(1).map(|d| d.lemma.clone()).unwrap_or_else(|| "beber".into()),
            distractors.get(2).map(|d| d.lemma.clone()).unwrap_or_else(|| "vivir".into()),
        ]);
        let ai = opts2.iter().position(|o| *o == w.lemma).unwrap_or(0) as i32;
        (
            Round::Choice {
                id,
                word_id: w.word_id,
                prompt: gloss,
                prompt_lang: "en".into(),
                options: opts2,
                answer_index: ai,
                audio_base64: ctx.audio(&w.lemma),
            },
            "choice",
        )
    } else {
        (
            Round::Choice {
                id,
                word_id: w.word_id,
                prompt: w.lemma.clone(),
                prompt_lang: "es".into(),
                options: opts,
                answer_index,
                audio_base64: ctx.audio(&w.lemma),
            },
            "choice",
        )
    }
}

/// Hear the word, pick it out. Without a speech engine there is nothing to
/// hear, so the word is quizzed in writing instead.
fn listen_round(ctx: &mut GenCtx, id: i64, w: &WordCard) -> (Round, &'static str) {
    if !ctx.tts.available() {
        return choice_round(ctx, id, w);
    }
    let distractors = ctx.content.similar_words(w, 8);
    let mut options = vec![w.lemma.clone()];
    for d in distractors.iter().take(3) {
        options.push(d.lemma.clone());
    }
    let opts = shuffle(options);
    let answer_index = opts.iter().position(|o| *o == w.lemma).unwrap_or(0) as i32;
    (
        Round::Listen {
            id,
            word_id: w.word_id,
            es: w.lemma.clone(),
            audio_base64: ctx.audio(&w.lemma),
            options: opts,
            answer_index,
        },
        "listen",
    )
}

/// The longest sentence given as a dictation, in characters. It has to be
/// held in the ear and typed out exactly. Common words rarely lack one this
/// short among the four example sentences looked at: 11 of the 2,000 most
/// frequent did when this was set.
const DICTATION_MAX_CHARS: usize = 40;

/// Hear a sentence with the word in it, write it down. `None` when there is
/// no speech engine (nothing could be heard) or no short sentence that this
/// session hasn't used yet.
fn dictation_round(ctx: &mut GenCtx, id: i64, w: &WordCard) -> Option<(Round, &'static str)> {
    if !ctx.tts.available() {
        return None;
    }
    let (sid, es, _) = ctx.content.sentences_for_word(w.word_id, 4).into_iter().find(|(sid, es, _)| {
        !ctx.used_sentences.contains(sid) && es.chars().count() <= DICTATION_MAX_CHARS
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
/// offers similar words in the same form as distractors.
fn cloze_round(
    ctx: &mut GenCtx,
    id: i64,
    w: &WordCard,
    es: &str,
    en: Option<&str>,
) -> Option<(Round, &'static str)> {
    let forms = ctx.content.forms_of(w.word_id);
    let (start, token, tag) = words_in(es).into_iter().find_map(|(start, token)| {
        let lower = token.to_lowercase();
        forms
            .iter()
            .find(|(f, _)| *f == lower)
            .map(|(_, tag)| (start, token, tag.clone()))
    })?;
    let target = token.to_lowercase();
    let mut options = vec![target.clone()];
    for d in ctx.content.similar_words(w, 12) {
        if options.len() == 4 {
            break;
        }
        if let Some(f) = ctx.content.form_with_tag(d.word_id, &tag) {
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
            sentence_en: en.unwrap_or("").to_string(),
            options: opts,
            answer_index,
        },
        "cloze",
    ))
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
        let candidates = content.words_by_rank(band, 6);
        let Some(w) = candidates.iter().find(|w| w.gloss_en.is_some()) else {
            continue;
        };
        let gloss = w.gloss_en.clone().unwrap_or_default();
        let distractors = content.similar_words(w, 12);
        let mut options = vec![gloss.clone()];
        for d in distractors.iter() {
            if let Some(g) = &d.gloss_en {
                if !options.contains(g) {
                    options.push(g.clone());
                }
            }
            if options.len() == 4 {
                break;
            }
        }
        let opts = shuffle(options);
        if let Some(ai) = opts.iter().position(|o| *o == gloss) {
            items.push(PlacementItem {
                word_id: w.word_id,
                lemma: w.lemma.clone(),
                options: opts,
                answer_index: ai as i32,
                band: w.level.clone(),
            });
        }
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
        with_ctx_speaking(Tts::off, f)
    }

    /// A voice that is there but never asked to speak: generating rounds
    /// only looks in the audio cache.
    fn voice(dir: &std::path::Path) -> Tts {
        Tts::with_espeak(dir, "/bin/true".into())
    }

    fn with_ctx_speaking<T>(tts: impl FnOnce(&std::path::Path) -> Tts, f: impl FnOnce(&mut GenCtx) -> T) -> T {
        let content = content();
        let (dir, user) = fresh_user();
        let tts = tts(&dir.0);
        let (mut used, mut introduced) = (HashSet::new(), HashSet::new());
        let mut ctx = GenCtx {
            content: &content,
            user: &user,
            tts: &tts,
            definition_lang: "en".into(),
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

    #[test]
    fn cloze_blanks_the_inflected_form_with_distractors_in_the_same_form() {
        with_ctx(|ctx| {
            let tener = word(ctx.content, "tener");
            let (round, _) =
                cloze_round(ctx, 0, &tener, "Yo tengo un perro muy grande.", None).unwrap();
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
            assert!(cloze_round(ctx, 0, &tener, "No hay nada aquí.", None).is_none());
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
        assert!(content
            .new_candidates(12_000, &HashSet::new())
            .iter()
            .chain(&content.verbs(2_000))
            .all(|w| !["mierda", "joder", "computadora"].contains(&w.lemma.as_str())));
    }

    #[test]
    fn distractors_never_mean_the_same_thing() {
        let content = content();
        let lemmas = |w: &str| -> Vec<String> {
            content.similar_words(&word(&content, w), 12).into_iter().map(|d| d.lemma).collect()
        };
        assert!(!lemmas("ser").iter().any(|l| l == "estar" || l == "haber"), "{:?}", lemmas("ser"));
        assert!(!lemmas("tu").iter().any(|l| l == "su"), "{:?}", lemmas("tu"));

        let parts = |g: &Option<String>| -> HashSet<String> {
            g.as_deref().unwrap_or("").split([';', ',']).map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect()
        };
        for w in content.words_by_rank(1, 300) {
            for d in content.similar_words(&w, 8) {
                assert!(parts(&w.gloss_en).is_disjoint(&parts(&d.gloss_en)), "{} / {}: {:?} vs {:?}", w.lemma, d.lemma, w.gloss_en, d.gloss_en);
            }
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
                examples.first().is_some_and(|(_, es, _)| too_long(es)) && examples.iter().any(|(_, es, _)| !too_long(es))
            })
            .take(5)
            .collect();
        assert!(!awkward.is_empty(), "no word in the dictionary has a long first example and a short later one");

        with_ctx_speaking(voice, |ctx| {
            for w in &awkward {
                let short: Vec<String> = ctx
                    .content
                    .sentences_for_word(w.word_id, 4)
                    .into_iter()
                    .map(|(_, es, _)| es)
                    .filter(|es| !too_long(es))
                    .collect();
                // each short example once, in order, and then no more
                for expected in &short {
                    let Some((Round::ListenType { word_id, sentence_es, answer, .. }, "listen_type")) = dictation_round(ctx, 0, w) else {
                        panic!("no dictation for a word with a short example")
                    };
                    assert_eq!((word_id, &sentence_es, &answer), (w.word_id, expected, expected));
                }
                assert!(dictation_round(ctx, 0, w).is_none(), "an example was dictated twice, or a long one was");
            }
        });
        // and never without a voice to say it
        with_ctx(|ctx| assert!(dictation_round(ctx, 0, &awkward[0]).is_none()));
    }
}
