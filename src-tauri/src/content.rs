//! Read-only access to the bundled content database.

use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use tilde_core::types::{ConjRow, WordCard};

pub struct ContentDb {
    pub conn: Connection,
}

impl ContentDb {
    /// Opens the bundled DB as immutable: installed resources live in read-only
    /// directories, where SQLite can't create the -shm file a WAL-mode database
    /// (as built by older pipelines) needs even for reads.
    pub fn open(path: &std::path::Path) -> ContentDb {
        let conn = Connection::open_with_flags(
            immutable_uri(path),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap_or_else(|e| panic!("open content db at {}: {e}", path.display()));
        ContentDb { conn }
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

    /// Newest teachable words at or after `frontier` rank. Vulgar words and
    /// ones only used outside Spain are never introduced.
    /// The next `limit` teachable words from `frontier` up, in frequency order,
    /// skipping `skip` (every word the player already has a card for). The
    /// frontier itself only moves at placement, so this is what keeps new
    /// words coming session after session.
    pub fn new_candidates(&self, frontier: i64, limit: usize, skip: &HashSet<i64>) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE rank >= ?1 AND gloss_en IS NOT NULL AND LENGTH(lemma) >= 2
                   AND register IS NULL AND region IS NULL
                 ORDER BY rank",
            )
            .unwrap();
        // rows are stepped lazily, so this reads only as far as it needs to
        stmt.query_map([frontier], word_from_row)
            .map(|rows| {
                rows.filter_map(|r| r.ok())
                    .filter(|w| !skip.contains(&w.word_id))
                    .take(limit)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn words_by_rank(&self, min_rank: i64, limit: i64) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE rank >= ?1 AND gloss_en IS NOT NULL AND LENGTH(lemma) >= 2
                   AND register IS NULL AND region IS NULL
                 ORDER BY rank LIMIT ?2",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![min_rank, limit], word_from_row)
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    pub fn similar_words(&self, word: &WordCard, limit: i64) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE id != ?1 AND gloss_en IS NOT NULL AND lemma != ?2
                   AND (pos IS ?3 OR pos IS NULL) AND ABS(rank - ?4) < 4000
                   AND register IS NULL AND gloss_en IS NOT (SELECT gloss_en FROM words WHERE id = ?1)
                 ORDER BY ABS(rank - ?4) LIMIT ?5",
            )
            .unwrap();
        stmt.query_map(
            rusqlite::params![word.word_id, word.lemma, word.pos, word.rank, limit],
            word_from_row,
        )
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
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

    /// Every known form (lemmas, plurals, conjugations) keyed accent-insensitively,
    /// for matching free text such as imported subtitles back to word ids.
    pub fn form_index(&self) -> HashMap<String, Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT form, word_id FROM forms").unwrap();
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        let mut map: HashMap<String, Vec<i64>> = HashMap::new();
        for (form, word_id) in rows {
            map.entry(tilde_core::srt::strip_accents(&form))
                .or_default()
                .push(word_id);
        }
        map
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

fn immutable_uri(path: &std::path::Path) -> String {
    let mut p = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        p = p.replace('\\', "/");
    }
    // only these three are special in the path part of an SQLite URI
    let p = p.replace('%', "%25").replace('?', "%3f").replace('#', "%23");
    format!("file:{p}?immutable=1")
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A WAL-mode DB in a read-only directory (as when installed) must still be
    /// readable. Only a non-root run exercises the permission part; the odd
    /// directory name also covers URI escaping either way.
    #[test]
    fn opens_wal_db_in_read_only_dir() {
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

        let lemma = std::panic::catch_unwind(|| ContentDb::open(&path).word(1).map(|w| w.lemma));

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(lemma.unwrap(), Some("hablar".to_string()));
    }
}
