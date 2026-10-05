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

pub fn to_hit(w: &WordCard, known: bool) -> tilde_core::types::WordHit {
    tilde_core::types::WordHit {
        word_id: w.word_id,
        lemma: w.lemma.clone(),
        pos: w.pos.clone(),
        rank: w.rank,
        gloss_en: w.gloss_en.clone(),
        gloss_es: w.gloss_es.clone(),
        level: w.level.clone(),
        known,
    }
}

pub struct Session {
    pub kind: SessionKind,
    pub rounds: Vec<Round>,
    pub round_types: Vec<&'static str>,
    pub results: Vec<(bool, i32)>, // (correct, quality)
    pub combo: i32,
    pub best_combo: i32,
    pub xp: i64,
    pub new_words: Vec<String>,
    pub started_at: f64,
    pub correct: i64,
    pub total_graded: i64,
    /// This session's row in `events`, created on the first answer.
    pub event_id: Option<i64>,
    /// Round indices already graded; a repeat submit is ignored.
    pub answered: HashSet<usize>,
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
    frontier: i64,
    used_sentences: &'a mut HashSet<i64>,
    introduced: &'a mut HashSet<i64>,
}

impl<'a> GenCtx<'a> {
    fn audio(&self, text: &str) -> Option<String> {
        self.tts.speak(text)
    }
}

pub struct Generated {
    pub rounds: Vec<Round>,
    pub round_types: Vec<&'static str>,
}

pub fn generate(
    kind: SessionKind,
    content: &ContentDb,
    user: &Connection,
    tts: &Tts,
    definition_lang: &str,
    frontier: i64,
) -> Generated {
    let n = round_count(kind);
    let mut used_sentences = HashSet::new();
    let mut introduced = HashSet::new();
    let mut ctx = GenCtx {
        content,
        user,
        tts,
        definition_lang: definition_lang.to_string(),
        frontier,
        used_sentences: &mut used_sentences,
        introduced: &mut introduced,
    };

    let (rounds, round_types) = match kind {
        SessionKind::Sidecar => gen_sidecar(&mut ctx, n),
        SessionKind::ReviewOnly => gen_reviews(&mut ctx, n),
        _ => gen_mixed(&mut ctx, kind, n),
    };
    Generated { rounds, round_types }
}

fn gen_reviews(ctx: &mut GenCtx, n: usize) -> (Vec<Round>, Vec<&'static str>) {
    let due = deck::due_card_ids(ctx.user, n as i64);
    let mut rounds = Vec::new();
    let mut types = Vec::new();
    for (i, wid) in due.into_iter().take(n).enumerate() {
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
    let due = deck::due_card_ids(ctx.user, n as i64);
    let learning = deck::learning_card_ids(ctx.user, (n * 2) as i64);
    let new: Vec<WordCard> = ctx
        .content
        .new_candidates(ctx.frontier, n as i64)
        .into_iter()
        .take(n / 2)
        .collect();

    let mut words: Vec<WordCard> = Vec::new();
    for wid in due.iter().chain(learning.iter()) {
        if let Some(w) = ctx.content.word(*wid) {
            words.push(w);
        }
    }
    words.extend(new);
    let words = pick(&words, n);

    let mut rounds = Vec::new();
    let mut types = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let gloss = gloss_of(w, &ctx.definition_lang);
        let distractors = ctx.content.similar_words(w, 8);
        let mut options = vec![gloss.clone()];
        for d in distractors.iter().take(3) {
            options.push(gloss_of(d, &ctx.definition_lang));
        }
        let opts = shuffle(options);
        let answer_index = opts.iter().position(|o| *o == gloss).unwrap_or(0) as i32;
        if i % 3 == 2 {
            // sentence dictation (easier variant: word with options)
            rounds.push(Round::Listen {
                id: i as i64,
                word_id: w.word_id,
                audio_base64: ctx.audio(&w.lemma),
                options: opts,
                answer_index,
            });
            types.push("listen");
        } else if i % 3 == 1 {
            let sentences = ctx.content.sentences_for_word(w.word_id, 3);
            let sent = sentences
                .iter()
                .find(|(sid, _, _)| !ctx.used_sentences.contains(sid));
            if let Some((sid, es, _)) = sent {
                ctx.used_sentences.insert(*sid);
                rounds.push(Round::ListenType {
                    id: i as i64,
                    word_id: w.word_id,
                    sentence_es: es.clone(),
                    audio_base64: ctx.audio(es),
                    answer: es.clone(),
                });
                types.push("listen_type");
                continue;
            }
            rounds.push(Round::Choice {
                id: i as i64,
                word_id: w.word_id,
                prompt: w.lemma.clone(),
                prompt_lang: "es".into(),
                options: opts,
                answer_index,
                audio_base64: ctx.audio(&w.lemma),
            });
            types.push("choice");
        } else {
            rounds.push(Round::Choice {
                id: i as i64,
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

fn gen_mixed(ctx: &mut GenCtx, kind: SessionKind, n: usize) -> (Vec<Round>, Vec<&'static str>) {
    let n_review = if kind == SessionKind::Quick { n / 3 } else { n * 2 / 5 };
    let n_new = (n / 5).max(2);
    let n_games = n - n_review - n_new;

    // gather pools
    let due = deck::due_card_ids(ctx.user, n_review as i64);
    let learning = deck::learning_card_ids(ctx.user, (n * 2) as i64);
    let knownish: HashSet<i64> = learning.iter().copied().collect();
    let new_words: Vec<WordCard> = ctx
        .content
        .new_candidates(ctx.frontier, n_new as i64)
        .into_iter()
        .filter(|w| !knownish.contains(&w.word_id))
        .take(n_new)
        .collect();

    let mut rounds: Vec<Round> = Vec::new();
    let mut types: Vec<&'static str> = Vec::new();
    let mut rid: i64 = 0;

    // intro cards for new words
    let mut intro_ids: HashSet<i64> = HashSet::new();
    let mut game_words: Vec<WordCard> = Vec::new();

    // new word blocks: intro + 2 games
    for w in &new_words {
        if intro_ids.insert(w.word_id) {
            deck::introduce(ctx.user, w.word_id);
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

    // learning games from knownish words
    let mut learn_words: Vec<WordCard> = learning
        .iter()
        .filter_map(|id| ctx.content.word(*id))
        .collect();
    if learn_words.len() < n_games {
        let extra: Vec<WordCard> = ctx
            .content
            .words_by_rank(ctx.frontier, (n_games as i64) * 2)
            .into_iter()
            .filter(|w| !knownish.contains(&w.word_id) && !game_words.iter().any(|g| g.word_id == w.word_id))
            .take(n_games - learn_words.len())
            .collect();
        learn_words.extend(extra);
    }

    let game_pool: Vec<WordCard> = {
        let mut all = game_words.clone();
        all.extend(learn_words);
        all
    };

    // conjugation rounds for verbs
    let verbs = ctx.content.verbs(60);
    let mut conj_used = 0usize;
    let max_conj = (n_games / 6).max(1);

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
                let (round, ty) = listen_round(ctx, rid, &w);
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
    let _ = head;
    let _ = shuffle::<i32>;
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

fn listen_round(ctx: &mut GenCtx, id: i64, w: &WordCard) -> (Round, &'static str) {
    let gloss = gloss_of(w, &ctx.definition_lang);
    let _ = &gloss;
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
            audio_base64: ctx.audio(&w.lemma),
            options: opts,
            answer_index,
        },
        "listen",
    )
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

pub fn placement_result(
    user: &Connection,
    content: &ContentDb,
    answers: &[PlacementAnswer],
) -> PlacementResult {
    let correct = answers.iter().filter(|a| a.correct).count();
    let cefr = match correct {
        0..=2 => "A1",
        3..=5 => "A2",
        6..=8 => "B1",
        _ => "B2",
    };
    let frontier: i64 = match correct {
        0..=2 => 30,
        3..=4 => 150,
        5..=6 => 400,
        7..=8 => 900,
        9..=10 => 1800,
        _ => 2800,
    };
    // mark everything below the frontier as already known (cap 800 words)
    let mut marked = 0i64;
    for w in content.words_by_rank(1, 4000) {
        if w.rank >= frontier.saturating_sub(60) || marked >= 800 {
            break;
        }
        if w.gloss_en.is_some() {
            deck::mark_known(user, w.word_id);
            marked += 1;
        }
    }
    crate::db::set_setting(user, "frontier_rank", &frontier.to_string());
    PlacementResult {
        frontier_rank: frontier,
        words_marked_known: marked,
        cefr_estimate: cefr.to_string(),
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
) -> ScoreOutcome {
    let mut earned_badges = Vec::new();

    if correct {
        *combo += 1;
        *best_combo = (*best_combo).max(*combo);
    } else {
        *combo = 0;
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
    if *combo >= 10 && award_badge(user, "combo-10") {
        earned_badges.push(find_badge("combo-10"));
    }
    if *combo >= 25 && award_badge(user, "combo-25") {
        earned_badges.push(find_badge("combo-25"));
    }

    ScoreOutcome {
        xp_gained: xp,
        combo: *combo,
        new_badges: earned_badges,
    }
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
pub fn award_badge(user: &Connection, id: &str) -> bool {
    user.execute(
        "INSERT OR IGNORE INTO badges(id, earned_at) VALUES (?1, ?2)",
        rusqlite::params![id, deck::now() as i64],
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

pub fn session_summary(
    user: &Connection,
    session: &Session,
    level_up: Option<LevelInfo>,
    new_badges: Vec<Badge>,
) -> SessionSummary {
    let minutes = (deck::now() - session.started_at) / 60.0;
    let accuracy = if session.total_graded > 0 {
        session.correct as f32 / session.total_graded as f32
    } else {
        0.0
    };
    let mut badges = new_badges;
    if session.total_graded >= 5 && (accuracy - 1.0).abs() < f32::EPSILON && award_badge(user, "perfect-session") {
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
    }

    fn with_ctx<T>(f: impl FnOnce(&mut GenCtx) -> T) -> T {
        let content = content();
        let dir = std::env::temp_dir();
        let user = crate::db::open(&dir.join(format!("tilde_session_test_{}.db", std::process::id()))).unwrap();
        let tts = Tts::discover(&dir);
        let (mut used, mut introduced) = (HashSet::new(), HashSet::new());
        let mut ctx = GenCtx {
            content: &content,
            user: &user,
            tts: &tts,
            definition_lang: "en".into(),
            frontier: 1,
            used_sentences: &mut used,
            introduced: &mut introduced,
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
            .new_candidates(1, 12_000)
            .iter()
            .chain(&content.verbs(2_000))
            .all(|w| !["mierda", "joder", "computadora"].contains(&w.lemma.as_str())));
    }
}
