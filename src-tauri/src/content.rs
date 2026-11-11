//! Read-only access to the bundled content database.

use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use tilde_core::types::{ConjRow, WordCard};

pub struct ContentDb {
    pub conn: Connection,
    ids: HashSet<i64>,
}

/// A word that may be taught: it has a meaning to show, and is neither vulgar
/// nor used only outside Spain. Every query that picks words to introduce, to
/// place by or to offer as a wrong answer goes through this, so they can't
/// disagree about which words those are. (The length test keeps stray single
/// letters out until the content database itself drops them.)
const TEACHABLE: &str =
    "(gloss_en IS NOT NULL AND LENGTH(lemma) >= 2 AND register IS NULL AND region IS NULL)";

impl ContentDb {
    /// Opens the bundled DB as immutable: installed resources live in read-only
    /// directories, where SQLite can't create the -shm file a WAL-mode database
    /// (as built by older pipelines) needs even for reads.
    pub fn open(path: &std::path::Path) -> Result<ContentDb, String> {
        let conn = Connection::open_with_flags(
            sqlite_file_uri(&path.to_string_lossy(), cfg!(windows)),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
        // opening is lazy; read something so a damaged file fails here, not
        // in the middle of a session
        conn.query_row("SELECT id FROM words LIMIT 1", [], |_| Ok(()))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let ids = conn
            .prepare("SELECT id FROM words")
            .and_then(|mut stmt| stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(ContentDb { conn, ids })
    }

    /// The id of every word there is. The file never changes while it is
    /// open, so this is read once. A card whose word isn't here (the word
    /// left the list in a later build) is kept, but must not be served or
    /// counted: see `deck`.
    pub fn word_ids(&self) -> &HashSet<i64> {
        &self.ids
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .ok()
    }

    pub fn word(&self, id: i64) -> Option<WordCard> {
        self.conn
            .query_row(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words WHERE id = ?1",
                [id],
                |r| {
                    Ok(WordCard {
                        word_id: r.get(0)?,
                        lemma: r.get(1)?,
                        pos: r.get(2)?,
                        rank: r.get(3)?,
                        gloss_en: r.get(4)?,
                        gloss_es: r.get(5)?,
                        level: tilde_core::cefr_for_rank(r.get::<_, i64>(3)?).to_string(),
                    })
                },
            )
            .ok()
    }

    pub fn word_ids_by_lemma(&self, lemma: &str) -> Vec<i64> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM words WHERE lemma = ?1 COLLATE NOCASE")
            .unwrap();
        stmt.query_map([lemma], |r| r.get(0))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    pub fn search(&self, query: &str, limit: i64) -> Vec<WordCard> {
        let q = format!("%{}%", query.to_lowercase().replace(['%', '_'], ""));
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE lemma LIKE ?1 OR LOWER(gloss_en) LIKE ?1
                 ORDER BY rank LIMIT ?2",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![q, limit], word_from_row)
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// The `limit` most frequent teachable words not in `skip` (every word
    /// the player already has a card for): what to teach next. Starting from
    /// the commonest word each time, rather than from a rank placement once
    /// set, is what leaves no gap between the words a player knows and the
    /// ones they are taught.
    pub fn new_candidates(&self, limit: usize, skip: &HashSet<i64>) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE {TEACHABLE} ORDER BY rank"
            ))
            .unwrap();
        // rows are stepped lazily, so this reads only as far as it needs to
        stmt.query_map([], word_from_row)
            .map(|rows| {
                rows.filter_map(|r| r.ok())
                    .filter(|w| !skip.contains(&w.word_id))
                    .take(limit)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Ids of the teachable words more frequent than `max_rank`: the ones
    /// placement marks as known.
    pub fn teachable_below(&self, max_rank: i64) -> Vec<i64> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT id FROM words WHERE rank < ?1 AND {TEACHABLE} ORDER BY rank"))
            .unwrap();
        stmt.query_map([max_rank], |r| r.get(0))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// Teachable words from `min_rank` up, for the placement questions.
    pub fn words_by_rank(&self, min_rank: i64, limit: i64) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE rank >= ?1 AND {TEACHABLE} ORDER BY rank LIMIT ?2"
            ))
            .unwrap();
        stmt.query_map(rusqlite::params![min_rank, limit], word_from_row)
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// Distractors for `word` from its own part of speech: teachable words of
    /// similar frequency with no meaning in common. Sharing any gloss part
    /// makes two answers both right: "ser" and "estar" are both "to be", "tu"
    /// and "su" both "your". For rounds where a word of another class would
    /// stand out (a noun offered for a gap that takes a verb).
    pub fn similar_words(&self, word: &WordCard, limit: i64) -> Vec<WordCard> {
        let meanings = gloss_parts(word.gloss_en.as_deref());
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE id != ?1 AND lemma != ?2 AND (pos IS ?3 OR pos IS NULL)
                   AND ABS(rank - ?4) < 4000 AND {TEACHABLE}
                 ORDER BY ABS(rank - ?4), rank"
            ))
            .unwrap();
        stmt.query_map(
            rusqlite::params![word.word_id, word.lemma, word.pos, word.rank],
            word_from_row,
        )
        .map(|rows| {
            rows.filter_map(|r| r.ok())
                .filter(|w| gloss_parts(w.gloss_en.as_deref()).is_disjoint(&meanings))
                .take(limit.max(0) as usize)
                .collect()
        })
        .unwrap_or_default()
    }

    /// Wrong answers to offer next to `word`, best first: teachable words
    /// with no meaning in common, those of its own part of speech before the
    /// rest, and the nearest in frequency first within each. Unlike
    /// `similar_words` this never runs dry: "sí" is the only word of its
    /// class and the articles all mean "the", and a question with one or two
    /// options is no question.
    pub fn distractor_candidates(&self, word: &WordCard, limit: usize) -> Vec<WordCard> {
        let meanings = gloss_parts(word.gloss_en.as_deref());
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE id != ?1 AND lemma != ?2 AND ABS(rank - ?4) < 4000 AND {TEACHABLE}
                 ORDER BY (pos IS NOT ?3), ABS(rank - ?4), rank"
            ))
            .unwrap();
        stmt.query_map(
            rusqlite::params![word.word_id, word.lemma, word.pos, word.rank],
            word_from_row,
        )
        .map(|rows| {
            rows.filter_map(|r| r.ok())
                .filter(|w| gloss_parts(w.gloss_en.as_deref()).is_disjoint(&meanings))
                .take(limit)
                .collect()
        })
        .unwrap_or_default()
    }

    pub fn sentences_for_word(&self, word_id: i64, limit: i64) -> Vec<(i64, String, Option<String>)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT s.id, s.es, s.en FROM sentences s
                 JOIN sentence_links l ON l.sentence_id = s.id
                 WHERE l.word_id = ?1 AND LENGTH(s.es) BETWEEN 15 AND 140
                 ORDER BY s.id LIMIT ?2",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![word_id, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Sentences containing words near a rank band (for build/cloze rounds).
    pub fn sentences_near(&self, max_rank: i64, limit: i64) -> Vec<(i64, i64, String, Option<String>)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT s.id, l.word_id, s.es, s.en FROM sentences s
                 JOIN sentence_links l ON l.sentence_id = s.id
                 JOIN words w ON w.id = l.word_id
                 WHERE w.rank <= ?1 AND LENGTH(s.es) BETWEEN 20 AND 90
                   AND s.en IS NOT NULL
                 ORDER BY s.id LIMIT ?2",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![max_rank, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// Most common verbs with a conjugation table, for conjugation rounds.
    pub fn verbs(&self, limit: i64) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT w.id, w.lemma, w.pos, w.rank, w.gloss_en, w.gloss_es FROM words w
                 WHERE w.pos = 'verb' AND w.register IS NULL AND w.region IS NULL
                   AND EXISTS (SELECT 1 FROM conjugations c WHERE c.word_id = w.id)
                 ORDER BY w.rank LIMIT ?1",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![limit], word_from_row)
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// Every known form (lemmas, plurals, conjugations), for matching free
    /// text such as imported subtitles back to word ids. Built on each call
    /// (about 0.2 s in a release build, 0.6 s in a debug one) rather than
    /// kept: it is some 200,000 strings, wanted only when subtitles are
    /// imported or a mined sentence is added to the deck.
    pub fn form_index(&self) -> FormIndex {
        // What each word brings to the choice between the words a form could
        // belong to. Read once, not joined to every form.
        let words: HashMap<i64, (String, i64, bool)> = self
            .conn
            .prepare(&format!("SELECT id, lemma, rank, {TEACHABLE} FROM words"))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        // One row per (form, word). A form can be listed for a word under
        // several tags, or under none: the empty tag is the word itself, but
        // also spellings that come with it without a place in its tables
        // ("dame" with "dar"), so only its lemma says which of them is the
        // word.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT form, word_id, MAX(tag != ''), MAX(tag != '' AND tag NOT LIKE 'imperativo|%')
                 FROM forms GROUP BY form, word_id",
            )
            .unwrap();
        let rows: Vec<(String, i64, bool, bool)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        let mut index = FormIndex { exact: HashMap::new(), folded: HashMap::new() };
        for (form, word_id, tagged, not_just_imperative) in rows {
            let Some((lemma, rank, teachable)) = words.get(&word_id) else {
                continue;
            };
            let fit = (form != *lemma, !teachable, !tagged, tagged && !not_just_imperative, *rank, word_id);
            // A form without accents is found as it is spelled, so only the
            // others need an entry to be found without theirs.
            let unaccented = tilde_core::srt::strip_accents(&form);
            if unaccented != form {
                keep_better(&mut index.folded, unaccented, fit);
            }
            keep_better(&mut index.exact, form, fit);
        }
        index
    }

    /// A verb (by its main part of speech) with a conjugation table. Nouns that
    /// happen to end in -ar/-er/-ir ("mujer", "lugar") are not.
    pub fn is_verb(&self, word_id: i64) -> bool {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM words w WHERE w.id = ?1 AND w.pos = 'verb'
                   AND EXISTS (SELECT 1 FROM conjugations c WHERE c.word_id = w.id))",
                [word_id],
                |r| r.get::<_, bool>(0),
            )
            .unwrap_or(false)
    }

    /// Wiktionary's conjugation table: gerundio and participio first, then
    /// each tense in person order.
    pub fn conjugations(&self, word_id: i64) -> Vec<ConjRow> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT tense, person, form FROM conjugations WHERE word_id = ?1
                 ORDER BY CASE tense
                     WHEN 'gerundio' THEN 0 WHEN 'participio' THEN 1 WHEN 'presente' THEN 2
                     WHEN 'pretérito' THEN 3 WHEN 'imperfecto' THEN 4 WHEN 'futuro' THEN 5
                     WHEN 'condicional' THEN 6 WHEN 'subjuntivo presente' THEN 7
                     WHEN 'subjuntivo imperfecto' THEN 8 WHEN 'imperativo' THEN 9 ELSE 10 END,
                   CASE person
                     WHEN '' THEN 0 WHEN 'yo' THEN 1 WHEN 'tú' THEN 2 WHEN 'él/ella/usted' THEN 3
                     WHEN 'nosotros' THEN 4 WHEN 'vosotros' THEN 5 ELSE 6 END",
            )
            .unwrap();
        stmt.query_map([word_id], |r| {
            Ok(ConjRow { tense: r.get(0)?, person: r.get(1)?, form: r.get(2)? })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    /// (form, tag) for every written form of a word; see the pipeline's
    /// `forms` table for the tag values.
    pub fn forms_of(&self, word_id: i64) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare("SELECT form, tag FROM forms WHERE word_id = ?1")
            .unwrap();
        stmt.query_map([word_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    /// The word's form with a given tag ("pretérito|yo", "plural"); the lemma
    /// itself for an empty tag.
    pub fn form_with_tag(&self, word_id: i64, tag: &str) -> Option<String> {
        if tag.is_empty() {
            return self.word(word_id).map(|w| w.lemma);
        }
        self.conn
            .query_row(
                "SELECT form FROM forms WHERE word_id = ?1 AND tag = ?2 ORDER BY form LIMIT 1",
                rusqlite::params![word_id, tag],
                |r| r.get(0),
            )
            .ok()
    }
}

/// The SQLite URI that opens `path` as an immutable file. `windows` says
/// which platform's paths to expect: a parameter, not a `cfg`, so the Windows
/// half is tested wherever the tests run.
fn sqlite_file_uri(path: &str, windows: bool) -> String {
    let mut p = path.to_string();
    if windows {
        // A verbatim path (`\\?\C:\...`, as Windows gives for an install
        // dir) can't go in as it is: its "?" lands where a URI has its
        // authority, and SQLite refuses to open it. SQLite puts the prefix
        // back itself for a path long enough to need it.
        if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
            p = format!(r"\\{rest}");
        } else if let Some(rest) = p.strip_prefix(r"\\?\") {
            p = rest.to_string();
        }
        p = p.replace('\\', "/");
        // a drive path is written "/C:/..." in an SQLite URI
        let mut start = p.chars();
        if start.next().is_some_and(|c| c.is_ascii_alphabetic()) && start.next() == Some(':') {
            p.insert(0, '/');
        }
    }
    // only these three are special in the path part of an SQLite URI
    let p = p.replace('%', "%25").replace('?', "%3f").replace('#', "%23");
    // A path starting "//" (a network share) needs the empty authority
    // spelled out, or its server name would be read as one.
    let scheme = if p.starts_with("//") { "file://" } else { "file:" };
    format!("{scheme}{p}?immutable=1")
}

fn word_from_row(r: &rusqlite::Row) -> rusqlite::Result<WordCard> {
    let rank: i64 = r.get(3)?;
    Ok(WordCard {
        word_id: r.get(0)?,
        lemma: r.get(1)?,
        pos: r.get(2)?,
        rank,
        gloss_en: r.get(4)?,
        gloss_es: r.get(5)?,
        level: tilde_core::cefr_for_rank(rank).to_string(),
    })
}

/// The separate meanings in a gloss: "to be; to be present" → {"be", "be present"}.
/// Without the "to" of an infinitive or of an indirect object, so that
/// "begin" and "to begin", or "them" and "to them", are one meaning.
pub(crate) fn gloss_parts(gloss: Option<&str>) -> HashSet<String> {
    gloss
        .unwrap_or("")
        .split([';', ','])
        .map(|p| p.trim().to_lowercase())
        .map(|p| p.strip_prefix("to ").map(|rest| rest.trim_start().to_string()).unwrap_or(p))
        .filter(|p| !p.is_empty())
        .collect()
}

/// Subtitle word lookup: each written form leads to the one word it most
/// likely belongs to. A card for every word a form could belong to would
/// teach "crear" to someone whose film said "creo".
///
/// A form that is itself a dictionary word is that word: "madre" is also
/// listed as the feminine of "padre", and "casa" as a form of "casar", but a
/// sentence saying "madre" or "casa" is about those words. Otherwise it goes
/// to a word that can be taught, for which it is a regular form rather than
/// a spelling only listed with it ("ves" is in the tables of "ver", and
/// merely listed with "ir"), and not only as an imperative ("sé" is "I know"
/// far more often than "be!"); among those, to the commonest.
///
/// Exact spelling wins over the accent-free fallback, so "mi" (my) and "mí"
/// (me) stay apart.
pub struct FormIndex {
    /// Each form, with the word that fits it best.
    exact: HashMap<String, Fit>,
    /// The forms that have accents, without them.
    folded: HashMap<String, Fit>,
}

impl tilde_core::srt::WordMatcher for FormIndex {
    fn lookup(&self, form: &str) -> Vec<i64> {
        self.exact
            .get(form)
            .or_else(|| self.folded.get(form))
            .map(|fit| vec![fit.5])
            .unwrap_or_default()
    }

    fn knows(&self, form: &str) -> bool {
        self.exact.contains_key(form)
    }
}

/// How well a word fits a written form, smallest first: (not its dictionary
/// form, not teachable, only listed with it, only an imperative, rank, word id).
/// The id is last: it settles a tie, and it is what a lookup answers with.
type Fit = (bool, bool, bool, bool, i64, i64);

/// Keeps under `form` whichever word fits it better.
fn keep_better(map: &mut HashMap<String, Fit>, form: String, fit: Fit) {
    map.entry(form).and_modify(|best| *best = fit.min(*best)).or_insert(fit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_uris_for_windows_paths_are_valid_sqlite_uris() {
        let program_files = "file:/C:/Program Files/Tilde/resources/content.db?immutable=1";
        let share = "file:////srv/share/T/content.db?immutable=1";
        let windows = [
            (r"C:\Program Files\Tilde\resources\content.db", program_files),
            (r"\\?\C:\Program Files\Tilde\resources\content.db", program_files),
            (r"\\?\UNC\srv\share\T\content.db", share),
            (r"\\srv\share\T\content.db", share),
            (r"resources\content.db", "file:resources/content.db?immutable=1"),
            (r"C:\a%b#c?d\x.db", "file:/C:/a%25b%23c%3fd/x.db?immutable=1"),
        ];
        for (path, uri) in windows {
            assert_eq!(sqlite_file_uri(path, true), uri, "{path}");
            // SQLite must take the URI for that file: open it if it is there
            // (the relative one is, when the tests run in src-tauri), and
            // otherwise object to nothing but its absence. A URI it can't
            // parse fails differently ("invalid uri authority").
            let there = [path.to_string(), path.replace('\\', "/")].iter().any(|p| std::path::Path::new(p).exists());
            let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI;
            match Connection::open_with_flags(uri, flags) {
                Ok(_) => assert!(there, "{uri} opened, but there is no {path}"),
                Err(e) => {
                    assert!(!there, "{uri} did not open {path}: {e}");
                    assert_eq!(e.sqlite_error_code(), Some(rusqlite::ErrorCode::CannotOpen), "{uri}: {e}");
                    assert!(!e.to_string().contains("authority"), "{uri}: {e}");
                }
            }
        }
        // off Windows a path is left as it is, backslashes and all
        assert_eq!(sqlite_file_uri("/home/a b/x.db", false), "file:/home/a b/x.db?immutable=1");
        assert_eq!(sqlite_file_uri(r"/home/a\b/x?.db", false), r"file:/home/a\b/x%3f.db?immutable=1");
        assert_eq!(sqlite_file_uri("resources/content.db", false), "file:resources/content.db?immutable=1");
    }

    /// A WAL-mode DB in a read-only directory (as when installed) must still be
    /// readable. Only a non-root run exercises the permission part; the odd
    /// directory name also covers URI escaping either way.
    #[cfg(unix)]
    #[test]
    fn opens_wal_db_in_read_only_dir() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tilde ro ?#%41 ñ {}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("content.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE words(id INTEGER PRIMARY KEY, lemma TEXT, pos TEXT, rank INTEGER,
                                    gloss_en TEXT, gloss_es TEXT);
                 INSERT INTO words VALUES (1, 'hablar', 'verb', 1, 'to speak', NULL);",
            )
            .unwrap();
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let lemma = ContentDb::open(&path).map(|c| c.word(1).map(|w| w.lemma));

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(lemma.unwrap(), Some("hablar".to_string()));
    }

    #[test]
    fn the_word_id_set_holds_every_word() {
        let content = bundled();
        let words: i64 = content.conn.query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0)).unwrap();
        assert!(words > 0);
        assert_eq!(content.word_ids().len() as i64, words);
        let (lowest, highest): (i64, i64) = content.conn.query_row("SELECT MIN(id), MAX(id) FROM words", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!(content.word_ids().contains(&lowest) && content.word_ids().contains(&highest));
        assert!(!content.word_ids().contains(&(highest + 1)));
    }

    fn bundled() -> ContentDb {
        ContentDb::open(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/content.db"))).unwrap()
    }

    #[test]
    fn a_gloss_is_split_into_its_meanings() {
        let parts = |gloss: &str| {
            let mut parts: Vec<String> = gloss_parts(Some(gloss)).into_iter().collect();
            parts.sort();
            parts
        };
        assert_eq!(parts("to be; to be present, To exist"), ["be", "be present", "exist"]);
        // "begin" is "to begin", and "them" is "to them"...
        assert_eq!(parts("to begin"), parts("begin; "));
        assert_eq!(parts("they, them"), ["them", "they"]);
        assert_eq!(parts("to them, for them"), ["for them", "them"]);
        // ...but "to" is a meaning itself, and so is what only starts with it
        assert_eq!(parts("to; at; today, tomorrow"), ["at", "to", "today", "tomorrow"]);
        assert!(gloss_parts(None).is_empty());
    }

    #[test]
    fn a_written_form_links_to_one_word() {
        use tilde_core::srt::WordMatcher;
        let content = bundled();
        let index = content.form_index();
        let lemma = |form: &str| -> Vec<String> {
            index.lookup(form).iter().map(|id| content.word(*id).unwrap().lemma).collect()
        };
        for (form, word) in [
            // a form of two verbs goes to the commoner one,
            ("creo", "creer"),
            // the plural to its noun, not to the noun's feminine,
            ("niños", "niño"),
            // a form in one word's tables to that word, not to a commoner
            // one it is only listed with,
            ("ves", "ver"),
            // a form that is one word's imperative and nothing else to the
            // word where it is more,
            ("sé", "saber"),
            // and a dictionary form to its own word, as spelled
            ("madre", "madre"),
            ("mi", "mi"),
            ("mí", "mí"),
        ] {
            assert_eq!(lemma(form), [word], "{form}");
        }
        // without its accent a form is still found, where no other word is spelled so
        assert_eq!(lemma("tambien"), ["también"]);
        assert!(index.knows("también") && !index.knows("tambien"));
        assert!(lemma("zzzz").is_empty());

        // never more than one, whatever the form
        let forms: Vec<String> = {
            let mut stmt = content.conn.prepare("SELECT DISTINCT form FROM forms").unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert!(forms.len() > 100_000);
        for form in &forms {
            assert_eq!(index.lookup(form).len(), 1, "{form}");
            assert!(index.knows(form), "{form}");
            assert!(index.lookup(&tilde_core::srt::strip_accents(form)).len() == 1, "{form} without accents");
        }
        // and a word's own name always leads to it
        let mut stmt = content.conn.prepare("SELECT id, lemma FROM words").unwrap();
        let words = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).unwrap();
        for (id, lemma) in words.map(|r| r.unwrap()) {
            assert_eq!(index.lookup(&lemma), [id], "{lemma}");
        }

        // The order of preference, over every form: the word it is the name
        // of, else a word that can be taught, else one that has it in its
        // tables. Each word a form is listed with, as (id, those three).
        let mut listed: HashMap<String, Vec<(i64, [bool; 3])>> = HashMap::new();
        let mut stmt = content
            .conn
            .prepare(&format!(
                "SELECT f.form, f.word_id, f.form = w.lemma, {TEACHABLE}, MAX(f.tag != '')
                 FROM forms f JOIN words w ON w.id = f.word_id GROUP BY f.form, f.word_id"
            ))
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, [r.get(2)?, r.get(3)?, r.get(4)?]))))
            .unwrap();
        for (form, word) in rows.map(|r| r.unwrap()) {
            listed.entry(form).or_default().push(word);
        }
        // how many forms each preference decided
        let mut decided = [0; 3];
        for (form, words) in &listed {
            let picked = index.lookup(form)[0];
            let (_, pick) = words.iter().find(|(id, _)| *id == picked).unwrap_or_else(|| panic!("{form} is not a form of the word it leads to"));
            let mut rivals: Vec<[bool; 3]> = words.iter().map(|(_, is)| *is).collect();
            for (i, count) in decided.iter_mut().enumerate() {
                if rivals.iter().any(|is| is[i]) {
                    assert!(pick[i], "{form}: preference {i} was passed over");
                    *count += usize::from(rivals.iter().any(|is| !is[i]));
                    rivals.retain(|is| is[i]);
                }
            }
        }
        assert!(decided.iter().all(|n| *n > 0), "a preference never decided anything: {decided:?}");
    }

    #[test]
    fn a_missing_or_damaged_content_db_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("tilde_content_bad_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(ContentDb::open(&dir.join("missing.db")).is_err());
        let junk = dir.join("junk.db");
        std::fs::write(&junk, b"this is not a database at all, just bytes").unwrap();
        assert!(ContentDb::open(&junk).is_err());
    }
}
