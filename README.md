# Tilde ~

[![CI](https://github.com/Tazwinator/Tilde/actions/workflows/ci.yml/badge.svg)](https://github.com/Tazwinator/Tilde/actions/workflows/ci.yml)

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
- **Words in order of use** — new words come commonest first. Each is
  shown on a card before it is ever asked, and "Ya me la sé" on the card
  files it as known.
- **Placement quiz** — ten questions, asked once, to mark the commonest
  words as known if you know them already. "No lo sé" beats guessing, and
  you can skip the quiz and start from zero.
- **Sidecar mode** ("Modo secundario") — a dimmed, keyboard-only quiz on
  the words you have already met, designed to run next to a film or series.
- **Sentence mining** — drop in a subtitle file (`.srt`, `.vtt` or plain
  text) from a show you watch; Tilde extracts sentences, matches them to
  the dictionary and can add the words to your deck.
- **Word explorer** — 12,000 frequency-ranked words (lemmas, so *tengo* and
  *tiene* count towards *tener*) with English glosses, Spanish definitions,
  example sentences and conjugation tables. A search finds a word by any of
  its forms, typed with or without accents.
- **Offline text-to-speech** — a real Castilian neural voice (Piper), with
  every word and sentence speakable. Without Piper, or if it stops working,
  Tilde uses `espeak-ng` when it's installed.
- **Stats without shame** — weekly minutes, XP, vocabulary growth curves.
- **Backup / restore** — one zip file, fully under your control. Tilde also
  writes one after every session, and if the progress file is ever damaged
  it sets it aside and recovers the newest backup by itself.

Everything is **offline**: the dictionary, sentence corpus and TTS engine
are all local. Your progress lives in a local SQLite database.

## Building

Prerequisites: Rust, Node.js, `webkit2gtk-4.1` (see
[Tauri prerequisites](https://tauri.app/start/prerequisites/)).

```sh
# 1. Install the TTS voice (optional but recommended; Linux x86_64/arm,
#    macOS). Downloads are pinned and checksum-verified.
./scripts/setup-tts.sh

# 2. Run in dev mode (hot reload + webview inspector). Only one Tilde runs
#    at a time: close an installed one first.
npm install
npm run tauri dev

# 3. Or build a release binary / installer
npm run tauri build
```

The word database (`src-tauri/resources/content.db`) is committed, so a
fresh clone builds and tests straight away. Rebuild it only when changing
the content pipeline or its curation files:

```sh
# a trial build: writes only the file you name
cargo run --release -p tilde-pipeline -- --out /tmp/content.db

# the real one: rewrites content.db and curation/word_ids.tsv
cargo run --release -p tilde-pipeline
```

The first build downloads about 200 MB of open data into `data/`. Each
build ends with a report of what it changed against the committed database:
read it before committing. A word's id is what a player's progress hangs
on, so `curation/word_ids.tsv` (the id of every word ever in the list) is
committed together with the database it was written for.

Hand-made corrections to the data live in `crates/tilde-pipeline/curation/`,
one file for each kind: English glosses and parts of speech
(`glosses.tsv`), reviewed Spanish definitions (`definiciones.tsv`), forms
that are or are not words of their own (`lemmas.txt`, `inflections.txt`),
names and other junk to keep out (`exclude.txt`), the vulgar and regional
flags (`vulgar.txt`, `not_regional.txt`), example sentences to leave out
(`bad_sentences.txt`) and English forms for telling which word an example
shows (`english_forms.tsv`). Each file starts with a comment that says how
to write a line.

Tests: `cargo test --workspace` and `npm test`; `npm run check` checks the
Svelte and TypeScript types. CI runs all three, plus a build of the UI,
`cargo clippy` and a `rustfmt` check of the pipeline crate (the only crate
formatted with it).

How the pieces fit together (the app, its two databases, sessions, speech,
backups and the content pipeline) is described in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
What was decided along the way and why, what has never been checked in the
real app and what is known to be imperfect is in
[docs/STATUS.md](docs/STATUS.md).

## Data sources & attribution

- Frequency list: [FrequencyWords](https://github.com/hermitdave/FrequencyWords)
  by Hermit Dave, from OpenSubtitles 2018 (CC BY-SA 4.0)
- Lemmas, English glosses, inflections and conjugation tables:
  [Wiktionary](https://en.wiktionary.org/) contributors, via
  [kaikki.org](https://kaikki.org/) (CC BY-SA 4.0). The glosses of about
  700 words are written by hand (`curation/glosses.tsv`).
- Spanish definitions: [Wikcionario](https://es.wiktionary.org/)
  contributors, via [kaikki.org](https://kaikki.org/) (CC BY-SA 4.0). Those
  of the 3,000 or so commonest words are reviewed in
  `curation/definiciones.tsv`, itself shared under CC BY-SA 4.0: most are
  Wikcionario's wording, the others were written for Tilde.
- Example sentences: [Tatoeba](https://tatoeba.org/) contributors via
  [OPUS](https://opus.nlpl.eu/) (CC BY 2.0 FR)
- TTS: [Piper](https://github.com/rhasspy/piper) with the
  `es_ES-davefx-medium` voice

## License

The code in this repository is released under the [MIT License](LICENSE).
The word database built from the data above (`src-tauri/resources/content.db`)
is shared under [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/);
see [its notice](src-tauri/resources/CONTENT_LICENSE.md).

## Other languages

Tilde teaches Castilian Spanish only. The content schema has a `lang`
column, but nothing reads it yet, and most of the rest is built around
Spanish:

- the pipeline maps Wiktionary's Spanish conjugation tables onto the app's
  tenses, and flags Latin-American and vulgar senses;
- the subtitle tokenizer, its stopword list and the answer checking (ñ as
  its own letter, accent slips) are Spanish-specific;
- the interface is written in Spanish, and the voice is Castilian.

Another language would need its own pipeline run and those parts
generalised: doable, but more than a configuration change.
