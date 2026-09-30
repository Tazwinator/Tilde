//! Offline text-to-speech: Piper (preferred) with espeak-ng fallback.
//! Synthesized audio is cached as wav files keyed by text+voice.

use base64::Engine;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Tts {
    pub engine: String,
    pub voice: String,
    piper: Option<(PathBuf, PathBuf, PathBuf)>, // (bin, model, config)
    espeak: Option<PathBuf>,
    cache_dir: PathBuf,
}

impl Tts {
    pub fn discover(app_dir: &Path) -> Tts {
        let cache_dir = app_dir.join("audio_cache");
        let _ = std::fs::create_dir_all(&cache_dir);

        let mut candidates: Vec<PathBuf> = Vec::new();
        candidates.push(app_dir.join("tts"));
        if let Ok(home) = std::env::var("HOME") {
            candidates.push(PathBuf::from(&home).join(".local/share/tilde/tts"));
        }
        for base in candidates {
            let piper = base.join("piper/piper");
            let model = base.join("voice/es_ES-davefx-medium.onnx");
            let conf = base.join("voice/es_ES-davefx-medium.onnx.json");
            if piper.exists() && model.exists() && conf.exists() {
                return Tts {
                    engine: "piper".into(),
                    voice: "es_ES-davefx-medium".into(),
                    piper: Some((piper, model, conf)),
                    espeak: which_espeak(),
                    cache_dir,
                };
            }
        }
        Tts {
            engine: if which_espeak().is_some() { "espeak-ng".into() } else { "none".into() },
            voice: "es".into(),
            piper: None,
            espeak: which_espeak(),
            cache_dir,
        }
    }

    pub fn available(&self) -> bool {
        self.piper.is_some() || self.espeak.is_some()
    }

    /// Returns base64-encoded wav audio for `text`, using the cache when possible.
    pub fn speak(&self, text: &str) -> Option<String> {
        if !self.available() || text.trim().is_empty() {
            return None;
        }
        let mut hasher = Sha1::new();
        hasher.update(self.engine.as_bytes());
        hasher.update(self.voice.as_bytes());
        hasher.update(text.trim().as_bytes());
        let key = hex::encode(hasher.finalize());
        let wav = self.cache_dir.join(format!("{key}.wav"));
        if !wav.exists() {
            let ok = match &self.piper {
                Some((bin, model, conf)) => synthesize_piper(bin, model, conf, text, &wav),
                None => synthesize_espeak(self.espeak.as_ref().unwrap(), text, &wav),
            };
            if !ok {
                let _ = std::fs::remove_file(&wav);
                return None;
            }
        }
        let bytes = std::fs::read(&wav).ok()?;
        Some(base64::engine::general_purpose::STANDARD.encode(bytes))
    }
}

fn which_espeak() -> Option<PathBuf> {
    for dir in ["/usr/bin", "/usr/local/bin"] {
        let p = Path::new(dir).join("espeak-ng");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn synthesize_piper(bin: &Path, model: &Path, conf: &Path, text: &str, out: &Path) -> bool {
    let tmp = out.with_extension("tmp.wav");
    let result = Command::new(bin)
        .args([
            "-m",
            model.to_string_lossy().as_ref(),
            "-c",
            conf.to_string_lossy().as_ref(),
            "-f",
            tmp.to_string_lossy().as_ref(),
            "--length-scale",
            "1.05",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.trim().as_bytes())?;
            child.wait()
        });
    match result {
        Ok(status) if status.success() && tmp.exists() => std::fs::rename(&tmp, out).is_ok(),
        _ => {
            let _ = std::fs::remove_file(&tmp);
            false
        }
    }
}

fn synthesize_espeak(bin: &Path, text: &str, out: &Path) -> bool {
    Command::new(bin)
        .args(["-v", "es", "-s", "150", "-w"])
        .arg(out)
        .arg(text.trim())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success() && out.exists())
        .unwrap_or(false)
}
