//! Build-time pipeline that constructs the offline content database
//! (src-tauri/resources/content.db) from open data sources.
//!
//! Sources:
//! - Frequency list: hermitdave/FrequencyWords (OpenSubtitles, CC-BY-SA 4.0)
//! - English glosses: FreeDict spa-eng (GPL) + MUSE en-es dictionary (CC-BY-SA)
//! - Sentences: Tatoeba via OPUS (CC-BY 2.0)
//!
//! Usage: cargo run --release -p tilde-pipeline [--out path] [--data-dir dir]

use regex::Regex;
use rusqlite::Connection;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const FREQ_URL: &str =
    "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/es/es_50k.txt";
const FREEDICT_URLS: [&str; 3] = [
    "https://freedict-storage.freementius.de/releases/2024.10.20/spa-eng.tei.gz",
    "https://freedict-storage.freementius.de/releases/2023.12.30/spa-eng.tei.gz",
    "https://download.freedict.de/spa-eng.tei.gz",
];
const MUSE_URL: &str = "https://dl.fbaipublicfiles.com/arrival/dictionaries/en-es.txt";
const OPUS_API: &str = "https://opus.nlpl.eu/opusapi/?corpus=Tatoeba&preprocessing=moses&lang1=en&lang2=es";

const MAX_WORDS: usize = 12000;
const MAX_SENTENCES: usize = 60000;

struct Args {
    out: PathBuf,
    data: PathBuf,
}

fn parse_args() -> Args {
    let mut out = PathBuf::from("src-tauri/resources/content.db");
    let mut data = PathBuf::from("data");
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next().map(PathBuf::from).unwrap_or(out),
            "--data-dir" => data = it.next().map(PathBuf::from).unwrap_or(data),
            other => eprintln!("unknown arg {other}"),
        }
    }
    Args { out, data }
}

fn download(url: &str, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        println!("  cached: {}", dest.display());
        return Ok(());
    }
    println!("  GET {url}");
    let resp = ureq::get(url.to_string())
        .header("user-agent", "tilde-pipeline/0.1")
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let mut reader = resp.into_body().into_reader();
    let mut buf = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| format!("read {url}: {e}"))?;
    std::fs::write(dest, &buf).map_err(|e| format!("write {}: {e}", dest.display()))?;
    println!("  saved {} ({} bytes)", dest.display(), buf.len());
    Ok(())
}

fn word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-záéíóúüñ]+$").unwrap())
}

fn sentence_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[^a-záéíóúüñA-ZÁÉÍÓÚÜÑ ]").unwrap())
}

struct FreqWord {
    lemma: String,
    rank: i64,
}

fn load_frequency(data: &Path) -> Vec<FreqWord> {
    let p = data.join("es_50k.txt");
    download(FREQ_URL, &p).expect("download frequency list");
    let text = std::fs::read_to_string(&p).expect("read frequency list");
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let w = line.split_whitespace().next().unwrap_or("").to_lowercase();
        if w.is_empty() || !word_re().is_match(&w) || !seen.insert(w.clone()) {
            continue;
        }
        out.push(FreqWord {
            rank: out.len() as i64 + 1,
            lemma: w,
        });
        if out.len() >= MAX_WORDS {
            break;
        }
    }
    out
}

/// Scan FreeDict TEI for (headword, translations, pos).
fn parse_freedict(xml: &str) -> HashMap<String, (Vec<String>, Option<String>)> {
    let mut map = HashMap::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<entry>") {
        let after = &rest[start + 7..];
        let Some(end) = after.find("</entry>") else { break };
        let entry = &after[..end];
        rest = &after[end + 8..];

        let orth = entry
            .split("<orth")
            .nth(1)
            .and_then(|s| s.split('>').nth(1))
            .and_then(|s| s.split('<').next())
            .map(|s| html_unescape(s.trim().to_lowercase()));
        let Some(orth) = orth else { continue };
        if !word_re().is_match(&orth) {
            continue;
        }
        let mut trs = Vec::new();
        let mut q = entry;
        while let Some(i) = q.find("<quote>") {
            let tail = &q[i + 7..];
            let Some(j) = tail.find("</quote>") else { break };
            let t = html_unescape(tail[..j].trim().to_string());
            if !t.is_empty() && t.chars().count() <= 40 {
                trs.push(t);
            }
            q = &tail[j + 8..];
            if trs.len() >= 3 {
                break;
            }
        }
        let pos = entry.find("type=\"pos\"").and_then(|i| {
            let tail = &entry[i..];
            let s = tail.find('>')? + 1;
            let e = tail[s..].find('<')? + s;
            Some(match tail[s..e].trim() {
                "noun" | "m" | "f" | "mf" | "n" => "noun".to_string(),
                "verb" | "v" => "verb".to_string(),
                "adj" => "adjective".to_string(),
                "adv" => "adverb".to_string(),
                _ => "other".to_string(),
            })
        });
        if !trs.is_empty() {
            map.entry(orth).or_insert((trs, pos));
        }
    }
    map
}

fn html_unescape(s: String) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// MUSE en-es: lines "english spanish" → spanish -> [english glosses]
fn parse_muse(text: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 {
            let (en, es) = (parts[0].to_lowercase(), parts[1].to_lowercase());
            let v = map.entry(es).or_default();
            if v.len() < 2 && !v.contains(&en) {
                v.push(en);
            }
        }
    }
    map
}

fn strip_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            'ü' => 'u',
            o => o,
        })
        .collect()
}

fn plural_of(word: &str) -> Option<String> {
    let last = word.chars().last()?;
    if word.len() < 3 {
        return None;
    }
    Some(match last {
        'a' | 'e' | 'o' | 'á' | 'é' | 'ó' => format!("{word}s"),
        'z' => format!("{}ces", &word[..word.len() - 1]),
        c if c.is_ascii_alphabetic() => format!("{word}es"),
        _ => return None,
    })
}

struct SentenceSource {
    es_lines: Vec<String>,
    en_lines: Vec<String>,
}

fn load_tatoeba(data: &Path) -> SentenceSource {
    let zip_path = data.join("tatoeba-en-es.zip");
    if !zip_path.exists() {
        // discover url from OPUS API
        println!("  discovering Tatoeba moses URL via OPUS API");
        let api = ureq::get(OPUS_API.to_string())
            .header("user-agent", "tilde-pipeline/0.1")
            .call()
            .expect("opus api")
            .body_mut()
            .read_to_string()
            .expect("opus api body");
        let url = api
            .split('"')
            .filter(|s| s.ends_with(".txt.zip") && s.contains("Tatoeba"))
            .next_back()
            .expect("no moses url found")
            .to_string();
        download(&url, &zip_path).expect("download tatoeba");
    }
    let f = std::fs::File::open(&zip_path).unwrap();
    let mut zip = zip::ZipArchive::new(f).unwrap();
    let mut es_lines = Vec::new();
    let mut en_lines = Vec::new();
    for name in ["Tatoeba.en-es.es", "Tatoeba.es-en.es"] {
        if let Ok(mut zf) = zip.by_name(name) {
            let mut s = String::new();
            zf.read_to_string(&mut s).unwrap();
            es_lines = s.lines().map(|l| l.to_string()).collect();
        }
    }
    for name in ["Tatoeba.en-es.en", "Tatoeba.es-en.en"] {
        if let Ok(mut zf) = zip.by_name(name) {
            let mut s = String::new();
            zf.read_to_string(&mut s).unwrap();
            en_lines = s.lines().map(|l| l.to_string()).collect();
        }
    }
    assert_eq!(es_lines.len(), en_lines.len(), "misaligned moses pair");
    SentenceSource { es_lines, en_lines }
}

fn main() {
    let args = parse_args();
    println!("== Tilde content pipeline ==");
    std::fs::create_dir_all(&args.data).unwrap();
    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    // ---- frequency words ----
    println!("[1/5] frequency list");
    let words = load_frequency(&args.data);

    // ---- glosses ----
    println!("[2/5] dictionaries");
    let fd_path = data_join(&args.data, "freedict-spa-eng.tei", &FREEDICT_URLS, true);
    let fd_xml = std::fs::read_to_string(&fd_path).unwrap();
    let freedict = parse_freedict(&fd_xml);
    println!("  FreeDict entries: {}", freedict.len());

    let muse_path = args.data.join("muse-en-es.txt");
    download(MUSE_URL, &muse_path).expect("download muse");
    let muse = parse_muse(&std::fs::read_to_string(&muse_path).unwrap());
    println!("  MUSE es entries: {}", muse.len());

    let mut glossed = 0;
    for w in &words {
        if let Some((_, _)) = freedict.get(&w.lemma) {
            glossed += 1;
        } else if muse.contains_key(&w.lemma) {
            glossed += 1;
        }
    }
    let cov = glossed as f32 / words.len() as f32;
    println!("  gloss coverage: {:.0}%", cov * 100.0);

    // ---- sentences ----
    println!("[3/5] sentences (Tatoeba via OPUS)");
    let src = load_tatoeba(&args.data);

    // word lookup: form -> word id is built after words insert; build freq set first
    let mut freq_index: HashMap<String, i64> = HashMap::new();
    for w in &words {
        freq_index
            .entry(strip_accents(&w.lemma))
            .or_insert(w.rank);
    }

    let mut picked: Vec<(String, String)> = Vec::new();
    let mut seen_es = std::collections::HashSet::new();
    // pass 1: sentences containing low-rank words get priority
    for (es, en) in src.es_lines.iter().zip(src.en_lines.iter()) {
        if picked.len() >= MAX_SENTENCES {
            break;
        }
        let es = es.trim();
        let en = en.trim();
        if es.is_empty() || en.is_empty() || seen_es.contains(es) {
            continue;
        }
        let wc = es.split_whitespace().count();
        if !(3..=18).contains(&wc) {
            continue;
        }
        let cleaned = sentence_word_re().replace_all(es, "");
        let lower = cleaned.to_lowercase();
        let has_freq = lower
            .split_whitespace()
            .any(|t| freq_index.contains_key(&strip_accents(t)) && freq_index[&strip_accents(t)] <= 3000);
        if !has_freq {
            continue;
        }
        seen_es.insert(es.to_string());
        picked.push((es.to_string(), en.to_string()));
    }
    println!("  sentences picked: {}", picked.len());

    // ---- build db ----
    println!("[4/5] writing {}", args.out.display());
    let _ = std::fs::remove_file(&args.out);
    let conn = Connection::open(&args.out).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE words(
           id INTEGER PRIMARY KEY,
           lang TEXT NOT NULL DEFAULT 'es',
           lemma TEXT NOT NULL,
           pos TEXT,
           rank INTEGER NOT NULL,
           gloss_en TEXT,
           gloss_es TEXT);
         CREATE UNIQUE INDEX idx_words_rank ON words(rank);
         CREATE INDEX idx_words_lemma ON words(lemma COLLATE NOCASE);
         CREATE TABLE forms(
           form TEXT NOT NULL,
           word_id INTEGER NOT NULL REFERENCES words(id),
           PRIMARY KEY(form, word_id));
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
         CREATE TABLE verbs(verb TEXT PRIMARY KEY, irregular INTEGER NOT NULL DEFAULT 0);",
    )
    .unwrap();

    conn.execute("BEGIN", []).unwrap();
    let mut word_id_of: HashMap<String, i64> = HashMap::new();
    let mut id_counter = 0i64;
    let mut stmt = conn
        .prepare("INSERT INTO words(id, lemma, pos, rank, gloss_en) VALUES (?1, ?2, ?3, ?4, ?5)")
        .unwrap();
    for w in &words {
        id_counter += 1;
        word_id_of.insert(w.lemma.clone(), id_counter);
        let gloss = if let Some((trs, _)) = freedict.get(&w.lemma) {
            Some(trs.join("; "))
        } else if let Some(ens) = muse.get(&w.lemma) {
            Some(ens.join("; "))
        } else {
            None
        };
        let pos = freedict
            .get(&w.lemma)
            .and_then(|(_, p)| p.clone())
            .or_else(|| {
                if tilde_core::conjugator::Conjugator::is_verb_like(&w.lemma) {
                    Some("verb".to_string())
                } else {
                    None
                }
            });
        stmt.execute(rusqlite::params![id_counter, w.lemma, pos, w.rank, gloss])
            .unwrap();
    }
    conn.execute("COMMIT", []).unwrap();

    // forms: lemmas, plurals, verb conjugations
    println!("[5/5] inflections & links");
    conn.execute("BEGIN", []).unwrap();
    {
        let mut stmt = conn
            .prepare("INSERT OR IGNORE INTO forms(form, word_id) VALUES (?1, ?2)")
            .unwrap();
        for w in &words {
            let id = word_id_of[&w.lemma];
            stmt.execute(rusqlite::params![w.lemma, id]).unwrap();
            let pos = freedict
                .get(&w.lemma)
                .and_then(|(_, p)| p.clone())
                .unwrap_or_default();
            if pos.is_empty() || pos == "noun" || pos == "adjective" {
                if let Some(pl) = plural_of(&w.lemma) {
                    stmt.execute(rusqlite::params![pl, id]).unwrap();
                }
            }
        }
    }
    conn.execute("COMMIT", []).unwrap();

    // verbs: conjugate and store forms
    let mut verb_count = 0usize;
    let mut irregular_count = 0usize;
    conn.execute("BEGIN", []).unwrap();
    {
        let mut stmt = conn
            .prepare("INSERT OR IGNORE INTO forms(form, word_id) VALUES (?1, ?2)")
            .unwrap();
        let mut vstmt = conn
            .prepare("INSERT OR REPLACE INTO verbs(verb, irregular) VALUES (?1, ?2)")
            .unwrap();
        for w in &words {
            let is_verb = tilde_core::conjugator::Conjugator::is_verb_like(&w.lemma);
            if !is_verb || w.rank > 3000 {
                continue;
            }
            let id = word_id_of[&w.lemma];
            if let Some(rows) = tilde_core::conjugator::Conjugator::conjugate(&w.lemma) {
                let irregular = tilde_core::conjugator::Conjugator::is_irregular(&w.lemma);
                vstmt.execute(rusqlite::params![w.lemma, irregular as i64]).unwrap();
                if irregular {
                    irregular_count += 1;
                }
                verb_count += 1;
                for r in rows {
                    stmt.execute(rusqlite::params![r.form, id]).unwrap();
                }
            }
        }
    }
    conn.execute("COMMIT", []).unwrap();
    println!("  verbs conjugated: {verb_count} (irregular: {irregular_count})");

    // form lookup for sentence links (accent-insensitive)
    let mut form_index: HashMap<String, Vec<i64>> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT form, word_id FROM forms").unwrap();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let form: String = row.get(0).unwrap();
            let id: i64 = row.get(1).unwrap();
            form_index.entry(strip_accents(&form)).or_default().push(id);
        }
    }

    // sentences + links
    conn.execute("BEGIN", []).unwrap();
    {
        let mut sstmt = conn
            .prepare("INSERT INTO sentences(es, en) VALUES (?1, ?2)")
            .unwrap();
        let mut lstmt = conn
            .prepare("INSERT OR IGNORE INTO sentence_links(sentence_id, word_id) VALUES (?1, ?2)")
            .unwrap();
        for (i, (es, en)) in picked.iter().enumerate() {
            let sid = (i + 1) as i64;
            sstmt.execute(rusqlite::params![es, en]).unwrap();
            let mut seen_ids = std::collections::HashSet::new();
            for token in tilde_core::srt::tokenize_es(es) {
                let key = strip_accents(&token);
                if let Some(ids) = form_index.get(&key) {
                    for wid in ids {
                        if seen_ids.insert(*wid) {
                            lstmt.execute(rusqlite::params![sid, wid]).unwrap();
                        }
                    }
                }
            }
        }
    }
    conn.execute("COMMIT", []).unwrap();

    // meta
    conn.execute(
        "INSERT INTO meta VALUES ('version','1'),
         ('built_at', datetime('now')),
         ('word_count', (SELECT COUNT(*) FROM words)),
         ('sentence_count', (SELECT COUNT(*) FROM sentences)),
         ('sources', 'Frequency data derived from OpenSubtitles via hermitdave/FrequencyWords (CC-BY-SA 4.0). English glosses from FreeDict spa-eng (GPL) and MUSE en-es (CC-BY-SA). Example sentences from Tatoeba via OPUS (CC-BY 2.0).')",
        [],
    )
    .unwrap();

    // Ship one self-contained file: a WAL-mode database needs -wal/-shm side
    // files, which can't be created next to a read-only installed resource.
    conn.execute_batch("PRAGMA journal_mode=DELETE; VACUUM;").unwrap();

    println!("done.");
}

/// Download helper supporting gzip for the FreeDict TEI.
fn data_join(data: &Path, name: &str, urls: &[&str], gunzip: bool) -> PathBuf {
    let dest = data.join(name);
    if dest.exists() {
        return dest;
    }
    let mut last_err = String::new();
    for url in urls {
        println!("  GET {url}");
        match ureq::get(url.to_string()).header("user-agent", "tilde-pipeline/0.1").call() {
            Ok(resp) => {
                let mut buf = Vec::new();
                resp.into_body()
                    .into_reader()
                    .read_to_end(&mut buf)
                    .expect("read body");
                if gunzip && url.ends_with(".gz") {
                    use std::io::Write;
                    let mut dec = flate2::read::GzDecoder::new(&buf[..]);
                    let mut out = Vec::new();
                    dec.read_to_end(&mut out).expect("gunzip");
                    let mut f = std::fs::File::create(&dest).unwrap();
                    f.write_all(&out).unwrap();
                } else {
                    std::fs::write(&dest, &buf).unwrap();
                }
                println!("  saved {} ({} bytes)", dest.display(), buf.len());
                return dest;
            }
            Err(e) => {
                last_err = format!("{url}: {e}");
                eprintln!("  failed: {last_err}");
            }
        }
    }
    panic!("all sources failed; last error: {last_err}");
}
