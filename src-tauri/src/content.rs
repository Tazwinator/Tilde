//! Read-only access to the bundled content database.

use rusqlite::Connection;
use std::collections::HashMap;
use tilde_core::types::WordCard;

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

    /// Newest teachable words at or after `frontier` rank.
    pub fn new_candidates(&self, frontier: i64, limit: i64) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE rank >= ?1 AND gloss_en IS NOT NULL AND LENGTH(lemma) >= 2
                 ORDER BY rank LIMIT ?2",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![frontier, limit * 4], word_from_row)
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default()
    }

    pub fn words_by_rank(&self, min_rank: i64, limit: i64) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, lemma, pos, rank, gloss_en, gloss_es FROM words
                 WHERE rank >= ?1 AND gloss_en IS NOT NULL AND LENGTH(lemma) >= 2
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

    /// Top verbs (rank <= 3000) usable for conjugation rounds.
    pub fn verbs(&self, limit: i64) -> Vec<WordCard> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT w.id, w.lemma, w.pos, w.rank, w.gloss_en, w.gloss_es FROM words w
                 JOIN verbs v ON v.verb = w.lemma
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
        let mut stmt = self.conn.prepare("SELECT form, word_id FROM forms").unwrap();
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

    pub fn is_verb(&self, word_id: i64) -> bool {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM verbs WHERE verb = (SELECT lemma FROM words WHERE id = ?1)",
                [word_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false)
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
