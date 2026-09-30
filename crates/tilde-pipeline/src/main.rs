//! Builds the offline content database (src-tauri/resources/content.db) from
//! open data. See `sources.rs` for the datasets and their licenses.
//!
//! Usage: cargo run --release -p tilde-pipeline [--out path] [--data-dir dir]
//! Downloads are cached in the data dir (default: `data/` at the repo root).

mod db;
mod lexicon;
mod sentences;
mod sources;

use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::{Path, PathBuf};

const MAX_WORDS: usize = 12_000;

const ATTRIBUTION: &str = "Word frequencies: hermitdave/FrequencyWords (OpenSubtitles 2018), \
CC BY-SA 4.0. Lemmas, glosses, inflections and conjugations: Wiktionary (en.wiktionary.org) \
via kaikki.org, CC BY-SA 4.0 and GFDL. Example sentences: Tatoeba (tatoeba.org) via OPUS, \
CC BY 2.0 FR.";

struct Args {
    out: PathBuf,
    data: PathBuf,
}

fn parse_args() -> Args {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = root.join("src-tauri/resources/content.db");
    let mut data = root.join("data");
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

fn gz(path: &Path) -> BufReader<flate2::read::GzDecoder<std::fs::File>> {
    let f = std::fs::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    BufReader::with_capacity(1 << 20, flate2::read::GzDecoder::new(f))
}

/// "word count" lines, most frequent first.
fn load_frequency(path: &Path) -> Vec<(String, u64)> {
    std::fs::read_to_string(path)
        .expect("read frequency list")
        .lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            Some((parts.next()?.to_lowercase(), parts.next()?.parse().ok()?))
        })
        .collect()
}

/// Hand-written glosses for function words whose Wiktionary glosses read badly.
fn gloss_overrides() -> HashMap<String, String> {
    include_str!("../curation/glosses.tsv")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t'))
        .map(|(w, g)| (w.trim().to_string(), g.trim().to_string()))
        .collect()
}

fn main() {
    let args = parse_args();
    println!("== Tilde content pipeline ==");
    std::fs::create_dir_all(&args.data).expect("create data dir");
    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent).expect("create output dir");
    }

    println!("[1/5] sources");
    let fetch = |s: &sources::Source| {
        sources::fetch(s, &args.data).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        })
    };
    let (freq_path, freq_sha) = fetch(&sources::FREQUENCY);
    let (wikt_path, wikt_sha) = fetch(&sources::WIKTIONARY);
    let (tatoeba_path, tatoeba_sha) = fetch(&sources::TATOEBA);

    println!("[2/5] indexing Wiktionary");
    let index = lexicon::Index::build(gz(&wikt_path));

    println!("[3/5] lemmatizing and ranking the frequency list");
    let ranked = index.rank(&load_frequency(&freq_path));
    let shortlist: HashSet<String> = ranked
        .iter()
        .take(MAX_WORDS + 2_000)
        .map(|(w, _)| w.clone())
        .collect();

    println!("[4/5] glosses, inflections and conjugation tables");
    let mut details = lexicon::details(gz(&wikt_path), &shortlist, &index, &gloss_overrides());
    let words: Vec<lexicon::Lemma> = ranked
        .iter()
        .filter_map(|(w, _)| details.remove(w))
        .take(MAX_WORDS)
        .collect();
    let verbs = words.iter().filter(|w| !w.conjugations.is_empty()).count();
    let latam: Vec<&str> = words
        .iter()
        .filter(|w| w.region.is_some())
        .map(|w| w.lemma.as_str())
        .collect();
    let vulgar: Vec<&str> = words
        .iter()
        .filter(|w| w.register.is_some())
        .map(|w| w.lemma.as_str())
        .collect();
    println!("  words: {}  verbs with tables: {verbs}", words.len());
    println!(
        "  top 40: {}",
        words
            .iter()
            .take(40)
            .map(|w| w.lemma.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!(
        "  latam-only ({}): {}",
        latam.len(),
        latam.iter().take(30).copied().collect::<Vec<_>>().join(" ")
    );
    println!(
        "  vulgar ({}): {}",
        vulgar.len(),
        vulgar
            .iter()
            .take(30)
            .copied()
            .collect::<Vec<_>>()
            .join(" ")
    );

    println!("[5/5] example sentences");
    let pairs = sentences::load_tatoeba(&tatoeba_path);
    let linker = sentences::Linker::new(
        words
            .iter()
            .enumerate()
            .flat_map(|(i, w)| w.forms.iter().map(move |(f, _)| (f.as_str(), i as i64 + 1))),
    );
    let picked = sentences::pick(&pairs, &linker);
    let with_examples: HashSet<i64> = picked
        .iter()
        .flat_map(|p| p.word_ids.iter().copied())
        .collect();
    println!(
        "  {} of {} pairs picked; {} words have examples",
        picked.len(),
        pairs.len(),
        with_examples.len()
    );

    let meta = [
        ("built_at", chrono_like_now()),
        ("word_count", words.len().to_string()),
        ("verb_count", verbs.to_string()),
        ("sentence_count", picked.len().to_string()),
        ("sources", ATTRIBUTION.to_string()),
        ("sha256_frequency", freq_sha),
        ("sha256_wiktionary", wikt_sha),
        ("sha256_tatoeba", tatoeba_sha),
    ];
    db::write(&args.out, &words, &picked, &meta).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    println!("wrote {}", args.out.display());
}

/// Seconds since the Unix epoch (avoids a date-time dependency for one field).
fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}
