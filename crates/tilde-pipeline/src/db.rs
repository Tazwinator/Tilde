//! Content database schema and writer.

use crate::lexicon::Lemma;
use crate::sentences::Picked;
use rusqlite::{params, Connection};
use std::path::Path;

pub const SCHEMA_VERSION: &str = "2";

const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE words(
  id INTEGER PRIMARY KEY,
  lang TEXT NOT NULL DEFAULT 'es',
  lemma TEXT NOT NULL,
  pos TEXT,
  rank INTEGER NOT NULL,
  gloss_en TEXT,
  gloss_es TEXT,
  region TEXT,   -- 'latam' when every sense is labelled as used outside Spain
  register TEXT  -- 'vulgar' when every sense is vulgar
);
CREATE UNIQUE INDEX idx_words_rank ON words(rank);
CREATE INDEX idx_words_lemma ON words(lemma COLLATE NOCASE);
CREATE TABLE forms(
  form TEXT NOT NULL,
  word_id INTEGER NOT NULL REFERENCES words(id),
  tag TEXT NOT NULL DEFAULT '',  -- '', 'plural', 'feminine plural', 'participio', 'tense|person'
  PRIMARY KEY(form, word_id, tag));
CREATE INDEX idx_forms_word ON forms(word_id);
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
/// Word ids equal frequency rank (1 = most common).
pub fn write(
    out: &Path,
    words: &[Lemma],
    sentences: &[Picked],
    meta: &[(&str, String)],
) -> Result<(), String> {
    let tmp = out.with_extension("db.tmp");
    let _ = std::fs::remove_file(&tmp);
    build(&tmp, words, sentences, meta).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, out).map_err(|e| format!("rename to {}: {e}", out.display()))
}

fn build(
    path: &Path,
    words: &[Lemma],
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
                "INSERT INTO words(id, lemma, pos, rank, gloss_en, region, register)
                 VALUES (?1, ?2, ?3, ?1, ?4, ?5, ?6)",
            )?;
            let mut form =
                tx.prepare("INSERT OR IGNORE INTO forms(form, word_id, tag) VALUES (?1, ?2, ?3)")?;
            let mut conj = tx.prepare(
                "INSERT INTO conjugations(word_id, tense, person, form) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (i, w) in words.iter().enumerate() {
                let id = i as i64 + 1;
                word.execute(params![id, w.lemma, w.pos, w.gloss, w.region, w.register])?;
                for (f, tag) in &w.forms {
                    form.execute(params![f, id, tag])?;
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
