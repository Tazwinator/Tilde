//! Builds the offline content database (src-tauri/resources/content.db) from
//! open data. See `sources.rs` for the datasets and their licenses.
//!
//! Usage: cargo run --release -p tilde-pipeline [--out path] [--data-dir dir]
//!        [--credits path]
//! Downloads are cached in the data dir (default: `data/` at the repo root).
//! `--credits` also writes how often each lemma's forms occur (lemma<TAB>count,
//! most frequent first), to judge curation by.
//!
//! A build to the default path also records the ids of new lemmas in
//! `curation/word_ids.tsv`; commit that file together with the database.
//! With `--out` (a trial build) the registry is read but left alone.

mod db;
mod definiciones;
mod ids;
mod lexicon;
mod sentences;
mod sources;

use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::{Path, PathBuf};

const MAX_WORDS: usize = 12_000;

/// Shown in the app's settings; every source requires attribution.
const ATTRIBUTION: &str = "Word frequencies: FrequencyWords by Hermit Dave \
(github.com/hermitdave/FrequencyWords), from OpenSubtitles 2018, CC BY-SA 4.0. \
Lemmas, glosses, inflections and conjugations: Wiktionary contributors (en.wiktionary.org) \
via kaikki.org, CC BY-SA 4.0. Spanish definitions: Wikcionario contributors (es.wiktionary.org) \
via kaikki.org, CC BY-SA 4.0. Example sentences: Tatoeba contributors (tatoeba.org) via OPUS, \
CC BY 2.0 FR. This word database is shared under CC BY-SA 4.0 \
(creativecommons.org/licenses/by-sa/4.0).";

struct Args {
    out: PathBuf,
    data: PathBuf,
    /// Building somewhere other than the committed database.
    trial: bool,
    credits: Option<PathBuf>,
}

fn parse_args() -> Args {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = root.join("src-tauri/resources/content.db");
    let mut data = root.join("data");
    let mut trial = false;
    let mut credits = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => {
                out = it.next().map(PathBuf::from).unwrap_or(out);
                trial = true;
            }
            "--data-dir" => data = it.next().map(PathBuf::from).unwrap_or(data),
            "--credits" => credits = it.next().map(PathBuf::from),
            other => eprintln!("unknown arg {other}"),
        }
    }
    Args {
        out,
        data,
        trial,
        credits,
    }
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

fn main() {
    let args = parse_args();
    println!("== Tilde content pipeline ==");
    std::fs::create_dir_all(&args.data).expect("create data dir");
    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent).expect("create output dir");
    }

    // read first: without the registry every word would get a new id
    let registry_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("curation/word_ids.tsv");
    let mut registry = ids::Registry::load(&registry_path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    // hand-written glosses for words whose Wiktionary glosses read badly
    let overrides = lexicon::parse_overrides(include_str!("../curation/glosses.tsv"))
        .unwrap_or_else(|e| {
            eprintln!("error: curation/glosses.tsv: {e}");
            std::process::exit(1);
        });

    // irregular English forms, to tell which reading a translation shows
    let english = sentences::English::parse(include_str!("../curation/english_forms.tsv"))
        .unwrap_or_else(|e| {
            eprintln!("error: curation/english_forms.tsv: {e}");
            std::process::exit(1);
        });
    // sentences judged wrong or misleading, never to be an example
    let blocked = sentences::blocklist(include_str!("../curation/bad_sentences.txt"));

    println!("[1/5] sources");
    let fetch = |s: &sources::Source| {
        sources::fetch(s, &args.data).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        })
    };
    let (freq_path, freq_sha) = fetch(&sources::FREQUENCY);
    let (wikt_path, wikt_sha) = fetch(&sources::WIKTIONARY);
    let (wikcionario_path, wikcionario_sha) = fetch(&sources::ES_WIKTIONARY);
    let (tatoeba_path, tatoeba_sha) = fetch(&sources::TATOEBA);

    println!("[2/5] indexing Wiktionary");
    let index = lexicon::Index::build(gz(&wikt_path));

    println!("[3/5] lemmatizing and ranking the frequency list");
    let ranked = index.rank(&load_frequency(&freq_path));
    if let Some(path) = &args.credits {
        let lines: String = ranked.iter().map(|(w, c)| format!("{w}\t{c}\n")).collect();
        std::fs::write(path, lines).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }
    let shortlist: HashSet<String> = ranked
        .iter()
        .take(MAX_WORDS + 2_000)
        .map(|(w, _)| w.clone())
        .collect();

    println!("[4/5] glosses, inflections and conjugation tables");
    let mut details = lexicon::details(gz(&wikt_path), &shortlist, &index, &overrides);
    let mut words: Vec<lexicon::Lemma> = ranked
        .iter()
        .filter_map(|(w, _)| details.remove(w))
        .take(MAX_WORDS)
        .collect();
    // needs the final list: a counterpart is a form until it is a word itself
    let counterparts = lexicon::drop_counterparts(&mut words);
    let wanted: HashMap<String, String> = words
        .iter()
        .map(|w| (w.lemma.clone(), w.pos.clone()))
        .collect();
    let curated = definiciones::curated(include_str!("../curation/definiciones.tsv"));
    let mut definitions =
        definiciones::spanish_definitions(gz(&wikcionario_path), &wanted, &curated);
    for w in &mut words {
        w.gloss_es = definitions.remove(&w.lemma);
    }
    let defined = words.iter().filter(|w| w.gloss_es.is_some()).count();
    // rows for words that have left the list
    let unused = definiciones::unused(&curated, &wanted);
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
    println!(
        "  words: {}  verbs with tables: {verbs}  Spanish definitions: {defined}",
        words.len()
    );
    println!("  counterpart forms dropped: {counterparts}");
    println!("  unused curated definitions: {}", unused.len());
    // every one of them: each is a row to take out of the file
    for row in unused.chunks(10) {
        println!("    {}", row.join(" "));
    }
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
    let word_ids = registry.assign(words.iter().map(|w| w.lemma.as_str()));
    let linker = sentences::Linker::new(
        words
            .iter()
            .zip(&word_ids)
            .map(|(w, id)| sentences::LinkWord {
                id: *id,
                lemma: &w.lemma,
                gloss: &w.gloss,
                forms: &w.forms,
                voseo: &w.voseo,
            }),
        english,
    );
    let picked = sentences::pick(&pairs, &linker, &blocked);
    let with_examples: HashSet<i64> = picked
        .iter()
        .flat_map(|p| p.links.iter().map(|l| l.word_id))
        .collect();
    println!(
        "  {} of {} pairs picked; {} words have examples",
        picked.len(),
        pairs.len(),
        with_examples.len()
    );
    let stale = sentences::not_found(&blocked, &pairs);
    println!(
        "  blocklisted sentences found: {} of {}",
        blocked.len() - stale.len(),
        blocked.len()
    );
    // each is a line to take out of the file
    for line in &stale {
        println!("    not in Tatoeba (curation/bad_sentences.txt): {line}");
    }
    let mut voseo: HashMap<String, usize> = HashMap::new();
    for pair in &pairs {
        if let Some(token) = linker.voseo(&sentences::tokens(&pair.es)) {
            *voseo.entry(token.to_string()).or_default() += 1;
        }
    }
    println!(
        "  pairs written with vos forms, never picked: {}",
        voseo.values().sum::<usize>()
    );
    // a name among these ("tomás") belongs in curation/exclude.txt
    let mut tokens: Vec<(&String, &usize)> = voseo.iter().collect();
    tokens.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let commonest: Vec<String> = tokens
        .iter()
        .take(8)
        .map(|(token, n)| format!("{token} {n}"))
        .collect();
    println!("    most often for: {}", commonest.join(", "));
    let (links, confirmed) = picked
        .iter()
        .flat_map(|p| &p.links)
        .fold((0, 0), |(n, c), l| (n + 1, c + usize::from(l.quality == 0)));
    println!("  links: {links}; the English shows the word's gloss in {confirmed}");
    // what the app can show: it leaves out very short and very long sentences
    let usable: HashSet<i64> = picked
        .iter()
        .filter(|p| (15..=140).contains(&p.es.chars().count()))
        .flat_map(|p| p.links.iter().map(|l| l.word_id))
        .collect();
    let without = |top: usize| -> Vec<&str> {
        words
            .iter()
            .zip(&word_ids)
            .take(top)
            .filter(|(_, id)| !usable.contains(id))
            .map(|(w, _)| w.lemma.as_str())
            .collect()
    };
    let (top_2000, top_5000) = (without(2_000), without(5_000));
    println!(
        "  without a usable example: {} of the top 2,000, {} of the top 5,000",
        top_2000.len(),
        top_5000.len()
    );
    for row in top_2000.chunks(10) {
        println!("    {}", row.join(" "));
    }

    let meta = [
        ("built_at", chrono_like_now()),
        ("word_count", words.len().to_string()),
        ("verb_count", verbs.to_string()),
        ("sentence_count", picked.len().to_string()),
        ("sources", ATTRIBUTION.to_string()),
        ("sha256_frequency", freq_sha),
        ("sha256_wiktionary", wikt_sha),
        ("sha256_wikcionario", wikcionario_sha),
        ("sha256_tatoeba", tatoeba_sha),
    ];
    // what the committed database had, to report what this build changes
    let committed =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/content.db");
    let before = db::read_words(&committed).unwrap_or_default();
    db::write(&args.out, &words, &word_ids, &picked, &meta).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    println!("wrote {}", args.out.display());

    let after: Vec<ids::Listed> = words
        .iter()
        .zip(&word_ids)
        .enumerate()
        .map(|(i, (w, id))| ids::Listed {
            lemma: w.lemma.clone(),
            id: *id,
            rank: i as i64 + 1,
            gloss: w.gloss.clone(),
        })
        .collect();
    print!("{}", ids::report(&before, &after));
    if args.trial {
        println!("trial build: curation/word_ids.tsv left as it was");
    } else {
        registry.save(&registry_path).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        });
        println!(
            "updated {}; commit it with the database",
            registry_path.display()
        );
    }
}

/// Seconds since the Unix epoch (avoids a date-time dependency for one field).
fn chrono_like_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}
