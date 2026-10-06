//! Offline text-to-speech: Piper (preferred) with espeak-ng fallback.
//! Synthesized audio is cached as wav files keyed by engine+voice+text.

use base64::Engine;
use sha1::{Digest, Sha1};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const VOICE: &str = "es_ES-davefx-medium";

/// Long enough for Piper's first run on a slow machine, which loads a 60 MB
/// model; short enough that a hung engine can't stall a session for good.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Piper is skipped after this many failures in a row, so a broken install
/// costs a couple of failed runs rather than one per word.
const PIPER_STRIKES: u32 = 2;

static TMP: AtomicUsize = AtomicUsize::new(0);

struct Piper {
    bin: PathBuf,
    model: PathBuf,
    config: PathBuf,
}

pub struct Tts {
    piper: Option<Piper>,
    espeak: Option<PathBuf>,
    piper_failures: AtomicU32,
    cache_dir: PathBuf,
    timeout: Duration,
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
        let piper = candidates.into_iter().find_map(|base| {
            let p = Piper {
                bin: base.join(format!("piper/piper{}", std::env::consts::EXE_SUFFIX)),
                model: base.join(format!("voice/{VOICE}.onnx")),
                config: base.join(format!("voice/{VOICE}.onnx.json")),
            };
            (p.bin.exists() && p.model.exists() && p.config.exists()).then_some(p)
        });
        Tts {
            piper,
            espeak: which_espeak(),
            piper_failures: AtomicU32::new(0),
            cache_dir,
            timeout: TIMEOUT,
        }
    }

    /// No engines: rounds carry no audio. For tests, which must not depend
    /// on (or wait for) whatever TTS the machine happens to have.
    pub fn off(app_dir: &Path) -> Tts {
        Tts {
            piper: None,
            espeak: None,
            piper_failures: AtomicU32::new(0),
            cache_dir: app_dir.join("audio_cache"),
            timeout: TIMEOUT,
        }
    }

    /// Speaks with `espeak` (in tests, a stand-in script) and nothing else.
    #[cfg(test)]
    pub(crate) fn with_espeak(app_dir: &Path, espeak: PathBuf) -> Tts {
        let cache_dir = app_dir.join("audio_cache");
        let _ = std::fs::create_dir_all(&cache_dir);
        Tts {
            espeak: Some(espeak),
            cache_dir,
            ..Tts::off(app_dir)
        }
    }

    fn working_piper(&self) -> Option<&Piper> {
        self.piper
            .as_ref()
            .filter(|_| self.piper_failures.load(Ordering::Relaxed) < PIPER_STRIKES)
    }

    /// The engine speaking right now (Piper until it proves broken).
    pub fn engine(&self) -> &'static str {
        if self.working_piper().is_some() {
            "piper"
        } else if self.espeak.is_some() {
            "espeak-ng"
        } else {
            "none"
        }
    }

    pub fn voice(&self) -> &'static str {
        if self.working_piper().is_some() {
            VOICE
        } else {
            "es"
        }
    }

    pub fn available(&self) -> bool {
        self.working_piper().is_some() || self.espeak.is_some()
    }

    /// Audio for `text` if it has already been synthesized by the current
    /// engine; never starts an engine, so it's safe on a hot path.
    pub fn cached(&self, text: &str) -> Option<String> {
        let text = text.trim();
        let (engine, voice) = if self.working_piper().is_some() {
            ("piper", VOICE)
        } else if self.espeak.is_some() {
            ("espeak-ng", "es")
        } else {
            return None;
        };
        read_b64(&self.wav_path(engine, voice, text))
    }

    /// Returns base64-encoded wav audio for `text`, using the cache when possible.
    pub fn speak(&self, text: &str) -> Option<String> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(piper) = self.working_piper() {
            let spoken = self.cached_or_make("piper", VOICE, text, |out| {
                let mut cmd = Command::new(&piper.bin);
                cmd.arg("-m")
                    .arg(&piper.model)
                    .arg("-c")
                    .arg(&piper.config)
                    .arg("-f")
                    .arg(out)
                    .args(["--length-scale", "1.05"]);
                run(cmd, text, self.timeout)
            });
            if spoken.is_some() {
                self.piper_failures.store(0, Ordering::Relaxed);
                return spoken;
            }
            let failures = self.piper_failures.fetch_add(1, Ordering::Relaxed) + 1;
            if failures == PIPER_STRIKES {
                eprintln!("piper failed {failures} times in a row; using espeak-ng instead");
            }
        }
        let espeak = self.espeak.as_ref()?;
        self.cached_or_make("espeak-ng", "es", text, |out| {
            // the text goes in on stdin: as an argument, text starting with
            // "-" would be read as an option
            let mut cmd = Command::new(espeak);
            cmd.args(["-v", "es", "-s", "150", "--stdin", "-w"]).arg(out);
            run(cmd, text, self.timeout)
        })
    }

    fn wav_path(&self, engine: &str, voice: &str, text: &str) -> PathBuf {
        let mut hasher = Sha1::new();
        hasher.update(engine.as_bytes());
        hasher.update(voice.as_bytes());
        hasher.update(text.as_bytes());
        self.cache_dir.join(format!("{}.wav", hex::encode(hasher.finalize())))
    }

    /// The cached wav for (engine, voice, text), synthesizing it with
    /// `synth(path)` on a miss.
    fn cached_or_make(&self, engine: &str, voice: &str, text: &str, synth: impl FnOnce(&Path) -> bool) -> Option<String> {
        let wav = self.wav_path(engine, voice, text);
        if !wav.exists() {
            // a private temp name, so two synths of the same text can't collide
            // and a killed run never leaves a truncated wav in the cache
            let mut tmp = wav.clone().into_os_string();
            tmp.push(format!(".{}-{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
            let tmp = PathBuf::from(tmp);
            let made = synth(&tmp) && tmp.exists() && std::fs::rename(&tmp, &wav).is_ok();
            let _ = std::fs::remove_file(&tmp);
            if !made {
                return None;
            }
        }
        read_b64(&wav)
    }
}

fn read_b64(wav: &Path) -> Option<String> {
    let bytes = std::fs::read(wav).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Runs `cmd` with `input` on stdin; false if it fails or outlives `timeout`
/// (then it is killed).
fn run(mut cmd: Command, input: &str, timeout: Duration) -> bool {
    let Ok(mut child) = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    // dropping stdin after the write is what tells the engine the text is done
    let wrote = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(input.as_bytes()).is_ok());
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return wrote && status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn which_espeak() -> Option<PathBuf> {
    let exe = format!("espeak-ng{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::var_os("PATH").unwrap_or_default();
    // apps started from a desktop launcher (macOS especially) don't get the
    // shell's PATH, so look in the usual package-manager places too
    std::env::split_paths(&path)
        .chain(["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from))
        .map(|dir| dir.join(&exe))
        .find(|p| p.is_file())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tilde_tts_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn script(path: &Path, body: &str) -> PathBuf {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_path_buf()
    }

    /// A stand-in espeak-ng that "speaks" by copying stdin to the -w file.
    fn fake_espeak(d: &Path) -> PathBuf {
        script(
            &d.join("espeak-ng"),
            r#"out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "-w" ]; then out="$2"; shift; fi
  shift
done
cat > "$out""#,
        )
    }

    fn tts(d: &Path, piper_body: Option<&str>, timeout: Duration) -> Tts {
        let piper = piper_body.map(|body| Piper {
            bin: script(&d.join("piper"), body),
            model: d.join("model.onnx"),
            config: d.join("model.onnx.json"),
        });
        let cache_dir = d.join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        Tts {
            piper,
            espeak: Some(fake_espeak(d)),
            piper_failures: AtomicU32::new(0),
            cache_dir,
            timeout,
        }
    }

    fn decoded(b64: Option<String>) -> String {
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64.expect("audio")).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn text_that_looks_like_an_option_is_spoken_not_parsed() {
        let d = dir("argv");
        let tts = tts(&d, None, TIMEOUT);
        assert_eq!(decoded(tts.speak("--version -v en hola")), "--version -v en hola");
    }

    #[test]
    fn a_broken_piper_hands_over_to_espeak() {
        let d = dir("broken");
        let calls = d.join("calls");
        let tts = tts(&d, Some(&format!("echo x >> '{}'\nexit 1", calls.display())), TIMEOUT);
        assert_eq!(tts.engine(), "piper");
        for word in ["uno", "dos", "tres", "cuatro"] {
            assert_eq!(decoded(tts.speak(word)), word);
        }
        let tried = std::fs::read_to_string(&calls).unwrap().lines().count();
        assert_eq!(tried as u32, PIPER_STRIKES, "kept retrying a broken piper");
        assert_eq!(tts.engine(), "espeak-ng");
    }

    #[test]
    fn a_hung_engine_is_killed_after_the_timeout() {
        let d = dir("hung");
        let tts = tts(&d, Some("sleep 30"), Duration::from_millis(300));
        let started = Instant::now();
        assert_eq!(decoded(tts.speak("hola")), "hola");
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }
}
