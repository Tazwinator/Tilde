//! Source datasets: where they come from and how they're verified.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct Source {
    pub name: &'static str,
    pub url: &'static str,
    pub file: &'static str,
    /// Expected SHA-256 for pinned downloads. Wiktionary is re-extracted weekly
    /// at a fixed URL, so it can't be pinned; its hash is recorded instead.
    pub sha256: Option<&'static str>,
}

pub const FREQUENCY: Source = Source {
    name: "FrequencyWords es_50k (OpenSubtitles 2018)",
    url: "https://raw.githubusercontent.com/hermitdave/FrequencyWords/525f9b560de45753a5ea01069454e72e9aa541c6/content/2018/es/es_50k.txt",
    file: "es_50k.txt",
    sha256: Some("dcff3ad4316192f4dc4ff7d26e637c6ff314ef1ca0f3f720c5649018a71056c0"),
};

pub const WIKTIONARY: Source = Source {
    name: "Wiktionary Spanish (kaikki.org extraction)",
    url: "https://kaikki.org/dictionary/Spanish/kaikki.org-dictionary-Spanish.jsonl.gz",
    file: "kaikki-spanish.jsonl.gz",
    sha256: None,
};

pub const TATOEBA: Source = Source {
    name: "Tatoeba en-es v2023-04-12 (OPUS)",
    url: "https://object.pouta.csc.fi/OPUS-Tatoeba/v2023-04-12/moses/en-es.txt.zip",
    file: "tatoeba-en-es.zip",
    sha256: Some("a26e7675cae64c93ce45e6cae48f38424bf26e0923cec9d7881b1fa2bb636227"),
};

/// Returns the local path and SHA-256 of `src`, downloading it into `data` if
/// needed. A cached file that doesn't match its pinned hash is an error.
pub fn fetch(src: &Source, data: &Path) -> Result<(PathBuf, String), String> {
    let dest = data.join(src.file);
    if !dest.exists() {
        println!("  {}: GET {}", src.name, src.url);
        let resp = ureq::get(src.url)
            .header("user-agent", "tilde-pipeline/0.2")
            .call()
            .map_err(|e| format!("{}: {e}", src.url))?;
        let mut buf = Vec::new();
        resp.into_body()
            .into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| format!("read {}: {e}", src.url))?;
        let part = dest.with_extension("part");
        std::fs::write(&part, &buf).map_err(|e| format!("write {}: {e}", part.display()))?;
        std::fs::rename(&part, &dest).map_err(|e| format!("rename {}: {e}", dest.display()))?;
    } else {
        println!("  {}: cached at {}", src.name, dest.display());
    }
    let hash = sha256_file(&dest)?;
    if let Some(expected) = src.sha256 {
        if hash != expected {
            return Err(format!(
                "{} has SHA-256 {hash}, expected {expected}; delete it to re-download",
                dest.display()
            ));
        }
    }
    Ok((dest, hash))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher).map_err(|e| format!("hash {}: {e}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}
