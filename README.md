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

- **Six game modes** feeding one smart review deck: word matching,
  multiple choice, listening drills, sentence building, cloze deletion and
  a full Castilian **verb conjugation trainer** (incl. vosotros), drilled
  from Wiktionary's conjugation tables.
- **Placement quiz** — a few questions to figure out what you already know.
- **Audio sidecar mode** — a dimmed, keyboard-only listening drill screen
  designed to run next to a film or series.
- **Sentence mining** — drop in an `.srt` subtitle file from a show you
  watch; Tilde extracts sentences, matches them to the dictionary and can
  add the words to your deck.
- **Word explorer** — 12,000 frequency-ranked words (lemmas, so *tengo* and
  *tiene* count towards *tener*) with English glosses, example sentences and
  conjugation tables.
- **Offline text-to-speech** — a real Castilian neural voice (Piper), with
  every word and sentence speakable. Without Piper, or if it stops working,
  Tilde uses `espeak-ng` when it's installed.
- **Stats without shame** — weekly minutes, XP, vocabulary growth curves.
- **Backup / restore** — one zip file, fully under your control.

Everything is **offline**: the dictionary, sentence corpus and TTS engine
are all local. Your progress lives in a local SQLite database.

## Building

Prerequisites: Rust, Node.js, `webkit2gtk-4.1` (see
[Tauri prerequisites](https://tauri.app/start/prerequisites/)).

```sh
# 1. Install the TTS voice (optional but recommended; Linux x86_64/arm,
#    macOS). Downloads are pinned and checksum-verified.
./scripts/setup-tts.sh

# 2. Run in dev mode (hot reload + webview inspector)
npm install
npm run tauri dev

# 3. Or build a release binary / installer
npm run tauri build
```

The word database (`src-tauri/resources/content.db`) is committed, so a
fresh clone builds and tests straight away. Rebuild it only when changing
the content pipeline:

```sh
cargo run --release -p tilde-pipeline   # downloads ~100 MB of open data into data/ once
```

Hand-made corrections to the data (gloss overrides, homographs) live in
`crates/tilde-pipeline/curation/`.

Tests: `cargo test --workspace` and `npm test`.

## Data sources & attribution

- Frequency list: [FrequencyWords](https://github.com/hermitdave/FrequencyWords)
  by Hermit Dave, from OpenSubtitles 2018 (CC BY-SA 4.0)
- Lemmas, English glosses, inflections and conjugation tables:
  [Wiktionary](https://en.wiktionary.org/) contributors, via
  [kaikki.org](https://kaikki.org/) (CC BY-SA 4.0)
- Example sentences: [Tatoeba](https://tatoeba.org/) contributors via
  [OPUS](https://opus.nlpl.eu/) (CC BY 2.0 FR)
- TTS: [Piper](https://github.com/rhasspy/piper) with the
  `es_ES-davefx-medium` voice

## License

The code in this repository is released under the [MIT License](LICENSE).
The word database built from the data above (`src-tauri/resources/content.db`)
is shared under [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/);
see [its notice](src-tauri/resources/CONTENT_LICENSE.md).

## Multi-language future

The content pipeline and schema are language-agnostic (`words.lang`,
per-language packs). New languages only need a new pipeline run — the app
itself is ready for them.
