//! Content database schema and writer.

use crate::lexicon::Lemma;
use crate::sentences::Picked;
use rusqlite::{params, Connection};
use std::path::Path;
use tilde_core::search_key;

pub const SCHEMA_VERSION: &str = "3";

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE words(
  id INTEGER PRIMARY KEY,
  lang TEXT NOT NULL DEFAULT 'es',
  lemma TEXT NOT NULL,
  lemma_key TEXT NOT NULL,  -- tilde_core::search_key(lemma): what a search is matched against
  pos TEXT,
  rank INTEGER NOT NULL,
  gloss_en TEXT,
  gloss_es TEXT,
  region TEXT,   -- 'latam' when every sense is labelled as used outside Spain
  register TEXT  -- 'vulgar' when every sense is vulgar
);
CREATE UNIQUE INDEX idx_words_rank ON words(rank);
CREATE UNIQUE INDEX idx_words_lemma ON words(lemma COLLATE NOCASE);
CREATE INDEX idx_words_key ON words(lemma_key);
CREATE TABLE forms(
  form TEXT NOT NULL,
  word_id INTEGER NOT NULL REFERENCES words(id),
  tag TEXT NOT NULL DEFAULT '',  -- '', 'plural', 'feminine plural', 'participio', 'tense|person'
  PRIMARY KEY(form, word_id, tag));
CREATE INDEX idx_forms_word ON forms(word_id);
CREATE TABLE form_keys(  -- search keys of forms whose key differs from the form itself
  key TEXT NOT NULL,
  word_id INTEGER NOT NULL REFERENCES words(id),
  PRIMARY KEY(key, word_id)) WITHOUT ROWID;
CREATE TABLE conjugations(
  word_id INTEGER NOT NULL REFERENCES words(id),
  tense TEXT NOT NULL,
  person TEXT NOT NULL,  -- '' for gerundio and participio
  form TEXT NOT NULL,
  PRIMARY KEY(word_id, tense, person));
CREATE TABLE sentences(
  id INTEGER PRIMARY KEY,
  lang TEXT NOT NULL DEFAULT 'es',
  es TEXT NOT NULL,
  en TEXT,
  tatoeba_id INTEGER);
CREATE TABLE sentence_links(
  sentence_id INTEGER NOT NULL REFERENCES sentences(id),
  word_id INTEGER NOT NULL REFERENCES words(id),
  PRIMARY KEY(sentence_id, word_id));
CREATE INDEX idx_sentence_links_word ON sentence_links(word_id);
";

/// Writes the database next to `out` and renames it into place once complete.
/// `words` are in frequency order, so `rank` is the position (1 = most
/// common); `ids[i]` is the stable id of `words[i]` (see `ids.rs`), which is
/// what forms, conjugations and sentence links refer to.
pub fn write(
    out: &Path,
    words: &[Lemma],
    ids: &[i64],
    sentences: &[Picked],
    meta: &[(&str, String)],
) -> Result<(), String> {
    if ids.len() != words.len() {
        return Err(format!("{} ids for {} words", ids.len(), words.len()));
    }
    let tmp = out.with_extension("db.tmp");
    let _ = std::fs::remove_file(&tmp);
    build(&tmp, words, ids, sentences, meta)
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, out).map_err(|e| format!("rename to {}: {e}", out.display()))
}

/// The word list of an existing content database (empty if there's none at
/// `path`), for the report of what a rebuild changes.
pub fn read_words(path: &Path) -> rusqlite::Result<Vec<crate::ids::Listed>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt =
        conn.prepare("SELECT lemma, id, rank, COALESCE(gloss_en, '') FROM words ORDER BY rank")?;
    let rows = stmt.query_map([], |r| {
        Ok(crate::ids::Listed {
            lemma: r.get(0)?,
            id: r.get(1)?,
            rank: r.get(2)?,
            gloss: r.get(3)?,
        })
    })?;
    rows.collect()
}

fn build(
    path: &Path,
    words: &[Lemma],
    ids: &[i64],
    sentences: &[Picked],
    meta: &[(&str, String)],
) -> rusqlite::Result<()> {
    {
        let mut conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;")?;
        conn.execute_batch(SCHEMA)?;
        let tx = conn.transaction()?;
        {
            let mut word = tx.prepare(
                "INSERT INTO words(
                   id, lemma, lemma_key, pos, rank, gloss_en, gloss_es, region, register)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut form =
                tx.prepare("INSERT OR IGNORE INTO forms(form, word_id, tag) VALUES (?1, ?2, ?3)")?;
            let mut form_key =
                tx.prepare("INSERT OR IGNORE INTO form_keys(key, word_id) VALUES (?1, ?2)")?;
            let mut conj = tx.prepare(
                "INSERT INTO conjugations(word_id, tense, person, form) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (i, (w, id)) in words.iter().zip(ids).enumerate() {
                let rank = i as i64 + 1;
                word.execute(params![
                    id,
                    w.lemma,
                    search_key(&w.lemma),
                    w.pos,
                    rank,
                    w.gloss,
                    w.gloss_es,
                    w.region,
                    w.register
                ])?;
                for (f, tag) in &w.forms {
                    form.execute(params![f, id, tag])?;
                    // a form that is its own key ("tengo") is found in
                    // `forms`; only the others ("tendrá") need a row
                    let key = search_key(f);
                    if key != *f {
                        form_key.execute(params![key, id])?;
                    }
                }
                for c in &w.conjugations {
                    conj.execute(params![id, c.tense, c.person, c.form])?;
                }
            }
            let mut sentence = tx.prepare("INSERT INTO sentences(es, en) VALUES (?1, ?2)")?;
            let mut link = tx.prepare(
                "INSERT OR IGNORE INTO sentence_links(sentence_id, word_id) VALUES (?1, ?2)",
            )?;
            for s in sentences {
                sentence.execute(params![s.es, s.en])?;
                let sid = tx.last_insert_rowid();
                for wid in &s.word_ids {
                    link.execute(params![sid, wid])?;
                }
            }
            let mut m = tx.prepare("INSERT INTO meta(key, value) VALUES (?1, ?2)")?;
            m.execute(params!["version", SCHEMA_VERSION])?;
            for (k, v) in meta {
                m.execute(params![k, v])?;
            }
        }
        tx.commit()?;
        // one self-contained file: WAL side files can't be created next to a
        // read-only installed resource
        conn.execute_batch("PRAGMA journal_mode=DELETE; VACUUM;")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lemma(word: &str) -> Lemma {
        Lemma {
            lemma: word.to_string(),
            pos: "noun".to_string(),
            gloss: "gloss".to_string(),
            gloss_es: None,
            region: None,
            register: None,
            forms: vec![(word.to_string(), String::new())],
            conjugations: Vec::new(),
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tilde_pipeline_{}_{name}.db", std::process::id()))
    }

    #[test]
    fn write_uses_given_ids_not_positions() {
        let out = scratch("ids");
        let words = [lemma("casa"), lemma("perro")];
        let picked = [Picked {
            es: "El perro duerme.".into(),
            en: "The dog sleeps.".into(),
            word_ids: vec![3],
        }];
        write(&out, &words, &[7, 3], &picked, &[]).unwrap();

        let conn = Connection::open(&out).unwrap();
        let rows = |sql: &str| -> Vec<(String, i64)> {
            let mut stmt = conn.prepare(sql).unwrap();
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(
            rows("SELECT lemma, id * 100 + rank FROM words ORDER BY rank"),
            [("casa".to_string(), 701), ("perro".to_string(), 302)]
        );
        assert_eq!(
            rows("SELECT form, word_id FROM forms ORDER BY form"),
            [("casa".to_string(), 7), ("perro".to_string(), 3)]
        );
        assert_eq!(
            rows("SELECT es, word_id FROM sentences JOIN sentence_links ON sentence_id = id"),
            [("El perro duerme.".to_string(), 3)]
        );
        assert_eq!(
            rows("SELECT value, 0 FROM meta WHERE key = 'version'"),
            [("3".to_string(), 0)]
        );
        drop(conn);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn writes_search_keys() {
        let out = scratch("keys");
        let mut nino = lemma("niño");
        nino.forms.push(("niños".to_string(), "plural".to_string()));
        let mut tener = lemma("tener");
        for (form, tag) in [
            ("tengo", "presente|yo"),
            ("tendrá", "futuro|él"),
            // one key for two forms of a word is one row
            ("tendrá", "futuro|usted"),
        ] {
            tener.forms.push((form.to_string(), tag.to_string()));
        }
        write(&out, &[nino, tener], &[186, 15], &[], &[]).unwrap();

        let conn = Connection::open(&out).unwrap();
        let rows = |sql: &str| -> Vec<(String, i64)> {
            let mut stmt = conn.prepare(sql).unwrap();
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(
            rows("SELECT lemma_key, id FROM words ORDER BY rank"),
            [("nino".to_string(), 186), ("tener".to_string(), 15)]
        );
        // only forms whose key differs from the form: "tengo" and "tener"
        // are found in `forms` as they are
        assert_eq!(
            rows("SELECT key, word_id FROM form_keys ORDER BY key"),
            [
                ("nino".to_string(), 186),
                ("ninos".to_string(), 186),
                ("tendra".to_string(), 15)
            ]
        );
        // a search by key has to use the index, not scan 12,000 lemmas
        let plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN SELECT id FROM words WHERE lemma_key = 'nino'",
                [],
                |r| r.get(3),
            )
            .unwrap();
        assert!(plan.contains("idx_words_key"), "{plan}");
        drop(conn);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn write_rejects_mismatched_ids_and_duplicate_lemmas() {
        let out = scratch("bad");
        assert!(write(&out, &[lemma("casa")], &[1, 2], &[], &[]).is_err());
        // the registry is keyed by lemma, so two rows for one lemma can't be allowed
        assert!(write(&out, &[lemma("casa"), lemma("Casa")], &[1, 2], &[], &[]).is_err());
        assert!(!out.exists());
        let _ = std::fs::remove_file(out.with_extension("db.tmp"));
    }
}
