# Tilde ~

A playful, zero-pressure Spanish trainer for Linux. Built to be picked up
whenever you feel like it — for three minutes or twenty — with no streaks,
no daily goals and no guilt mechanics. Absence costs nothing; progress is
yours forever.

## Why another language app?

Duolingo-style apps lean on streak anxiety and drill-heavy exercises, which
research on second-language acquisition suggests is exactly the wrong
emotional climate for learning (stress blocks acquisition). Tilde is built
on two evidence-based ideas:

- **Spaced repetition (FSRS)** — the modern Anki algorithm. It is
  mathematically gap-tolerant: come back after three weeks and it quietly
  reschedules. There is never a backlog punishing you.
- **Positive, cumulative gamification** — XP, levels, badges and combos are
  earned once and never lost. No leaderboards, no daily quests, no debt.

## Features

- **Five game modes** feeding one smart review deck: word matching,
  multiple choice, listening drills, sentence building, cloze deletion and
  a full Castilian **verb conjugation trainer** (incl. vosotros).
- **Placement quiz** — a few questions to figure out what you already know.
- **Audio sidecar mode** — a dimmed, keyboard-only listening drill screen
  designed to run next to a film or series.
- **Sentence mining** — drop in an `.srt` subtitle file from a show you
  watch; Tilde extracts sentences, matches them to the dictionary and can
  add the words to your deck.
- **Word explorer** — 12,000+ frequency-ranked words with English glosses,
  example sentences and conjugation tables.
- **Offline text-to-speech** — a real Castilian neural voice (Piper), with
  every word and sentence speakable.
- **Stats without shame** — weekly minutes, XP, vocabulary growth curves.
- **Backup / restore** — one zip file, fully under your control.

Everything is **offline**: the dictionary, sentence corpus and TTS engine
are all local. Your progress lives in a local SQLite database.

## Building

Prerequisites: Rust, Node.js, `webkit2gtk-4.1` (see
[Tauri prerequisites](https://tauri.app/start/prerequisites/)).

```sh
# 1. Build the offline content database (downloads open data once)
cargo run --release -p tilde-pipeline

# 2. Install the TTS voice (optional but recommended)
./scripts/setup-tts.sh

# 3. Run in dev mode
npm install
npm run tauri dev

# 4. Or build a release binary / installer
npm run tauri build
```

## Data sources & attribution

- Frequency list: [OpenSubtitles via FrequencyWords](https://github.com/hermitdave/FrequencyWords) (CC-BY-SA 4.0)
- English glosses: [FreeDict spa-eng](https://freedict.org/) (GPL) and the
  [MUSE en-es bilingual dictionary](https://github.com/facebookresearch/MUSE) (CC-BY-SA)
- Example sentences: [Tatoeba](https://tatoeba.org/) via
  [OPUS](https://opus.nlpl.eu/) (CC-BY 2.0)
- TTS: [Piper](https://github.com/rhasspy/piper) with the
  `es_ES-davefx-medium` voice

## License

All original code in this repository is proprietary for now; it will be
re-licensed MIT when the project goes public. Bundled third-party data
keeps its own licenses (see attribution above).

## Multi-language future

The content pipeline and schema are language-agnostic (`words.lang`,
per-language packs). New languages only need a new pipeline run — the app
itself is ready for them.
