# Architecture

How Tilde is put together: what runs where, where data lives, and the rules
the code keeps. Read this before a change that crosses a module boundary.
For building and running, see the [README](../README.md).

## The shape of it

Tilde is a desktop app with no server. A [Tauri 2](https://tauri.app/) shell
written in Rust hosts a web UI (SvelteKit and Svelte 5) in the system
webview, and the two talk only through Tauri commands. Nothing goes over the
network at runtime: the dictionary, the example sentences and the voice are
all on disk.

```
 ┌───────────────────────── webview ─────────────────────────┐
 │  SvelteKit SPA (src/)                                     │
 │  routes: home, play, sidecar, explore, sentences, stats,  │
 │          placement, settings                              │
 │  lib/api.ts ── typed invoke() wrapper, 30 s timeout       │
 └────────────────────────────┬──────────────────────────────┘
                              │ Tauri commands (JSON)
 ┌────────────────────────────▼──────────────────────────────┐
 │  Rust app (src-tauri/src)                                 │
 │  lib.rs      commands, AppState, startup                  │
 │  session.rs  round generation, scoring, badges            │
 │  deck.rs     FSRS scheduling      content.rs  word lookup │
 │  db.rs       user DB + migrations backup.rs   zip backups │
 │  tts.rs      Piper / espeak-ng, wav cache                 │
 └──────┬──────────────────────┬─────────────────────┬───────┘
        │ read-only            │ read-write          │ child process
  content.db              tilde_user.db         piper or espeak-ng
  (words, forms,          (cards, reviews,      (optional; rounds
   conjugations,           events, badges,       work silently
   sentences)              mined, settings)      without them)
        ▲
        │ built offline by crates/tilde-pipeline from open data
```

## Repository map

| Path | What it is |
| --- | --- |
| `src/` | The UI: SvelteKit in SPA mode (`adapter-static`, `ssr = false`) |
| `src-tauri/` | The app crate (`tilde`): commands, state, the user database |
| `src-tauri/resources/content.db` | The word database, committed and bundled |
| `crates/tilde-core/` | Shared types (the wire format), XP levels, badges, subtitle parsing |
| `crates/tilde-pipeline/` | Builds `content.db` from open data; not shipped |
| `crates/tilde-pipeline/curation/` | Hand-made corrections fed into the pipeline |
| `scripts/setup-tts.sh` | Installs Piper and the Castilian voice (pinned, checksummed) |
| `tests/` | Frontend unit tests (`node --test`) |
| `.github/workflows/ci.yml` | CI: checks, tests, clippy, formatting |

## Runtime

### State and threads

All app state is one `AppState` (`src-tauri/src/lib.rs`), managed by Tauri:

- `content`: the open `content.db` (`ContentDb`), behind a mutex.
- `user`: the open `tilde_user.db` connection, behind a mutex.
- `tts`: the speech engines, shared (`Arc`) with background threads.
- `sessions`: sessions in progress, by id, in memory only.
- `app_dir`: the per-user data directory.

Every command is `#[tauri::command(async)]`, so it runs on Tauri's thread
pool. A slow command (speech, a large subtitle file, a backup) never blocks
the window. Two rules keep this safe:

- **Lock order is sessions → user → content.** A command that needs more
  than one lock takes them in that order. Helpers take `&Connection` rather
  than `&AppState`, because `std::sync::Mutex` deadlocks if the same thread
  locks it twice.
- **Locks are taken with `.locked()`**, which recovers a poisoned mutex. A
  command that panics mid-way (SQLite rolls its statement back) must not
  take every later command down with it.

### Startup

`run()` resolves the app data directory, then:

1. Opens `content.db`. If it is missing or unreadable, a native dialog
   explains that reinstalling fixes it, and the app exits.
2. Opens `tilde_user.db` with `db::open_or_set_aside`. A file SQLite can't
   read is renamed to `tilde_user.db.damaged-<time>`. The player starts
   fresh and is told where the old file is, so nothing is deleted.
3. Discovers the speech engines (`Tts::discover`).

### Commands

The UI sees only these commands, all in `lib.rs` and wrapped one-to-one in
`src/lib/api.ts`:

| Area | Commands |
| --- | --- |
| Profile and settings | `profile_get`, `settings_get`, `settings_set`, `badges_list` |
| Play | `session_start`, `round_submit`, `session_finish` |
| Placement | `placement_status`, `placement_start`, `placement_submit` |
| Words | `word_search`, `word_detail`, `word_mark_known`, `word_reset` |
| Speech | `tts_speak`, `tts_info` |
| Sentence mining | `srt_import`, `sentences_list`, `sentence_add_to_deck` |
| Stats and credits | `stats_get`, `content_credits` |
| Backups | `backup_folder`, `backup_folder_set`, `backup_export`, `backup_import`, `backup_restore`, `progress_reset` |

Their argument and result types live in `crates/tilde-core/src/types.rs`
(serde, camelCase on the wire) and are mirrored by hand in
`src/lib/contract.ts`. A test (`round_wire_format_matches_contract_ts`)
reads `contract.ts` and fails if the `Round` variants drift apart.

## Data

### `content.db`: what there is to learn

Read-only, and opened with SQLite's `immutable=1`. It is built by the
pipeline, committed to the repository and bundled as a Tauri resource.

| Table | Holds |
| --- | --- |
| `words` | One row per lemma. Its `id` is its frequency rank (1 = most common). Also `pos`, `gloss_en`, `gloss_es`, and `region` and `register` flags for Latin-American-only and vulgar words |
| `forms` | Every written form and the lemma it belongs to, with a tag (`plural`, `participio`, `tense\|person`) |
| `conjugations` | Verb tables from Wiktionary: `(word_id, tense, person) → form` |
| `sentences`, `sentence_links` | Tatoeba es–en pairs and the words each one contains |
| `meta` | Build time, counts, attribution text and the sha256 of each source |

`content.rs` wraps the queries: search, new-word candidates by rank,
distractors (`similar_words`), example sentences, conjugations, and the
`FormIndex` that maps any written form back to its lemmas for sentence
mining.

### `tilde_user.db`: the player's progress

Lives in the app data directory, in WAL mode with foreign keys on. All
progress is in this one file, which is why backups are simple.

| Table | Holds |
| --- | --- |
| `cards` | FSRS state per word the player has met: stability, difficulty, due time, reps, lapses |
| `reviews` | Every grade given, for history and stats |
| `events` | One row per session: XP, duration, kind. Total XP is `SUM(xp)` |
| `round_log` | One row per answered round, for per-skill badges and stats |
| `badges` | Earned badges and when |
| `mined`, `mined_words` | Sentences imported from subtitles and the words in each |
| `settings` | Key/value: definition language, session length, sound, placement result, frontier rank |

**Migrations.** `db::MIGRATIONS` is an append-only list of SQL steps.
`PRAGMA user_version` records how many have run. `db::open` runs the rest in
one transaction, and refuses a database from a newer Tilde rather than
guessing. To change the schema, append a step: never edit or reorder an old
one.

## How a session works

1. **`session_start`** calls `session::generate`, which builds every round
   up front from the deck and the content DB:
   - `gen_mixed` (Quick, Standard, Deep) mixes due reviews, a few new words
     (each an intro card followed by quizzes), learning games, conjugation
     drills and match rounds. It then shuffles the games but keeps intro
     blocks near the start.
   - `gen_reviews` (Solo repaso) serves only due cards, as review cards.
   - `gen_sidecar` serves keyboard-only listening and choice rounds.

   New words are the most frequent words above the player's *frontier* (set
   by placement) that have no card yet.

2. **Audio** in generated rounds is cache-only, so generation never waits
   for a speech engine. Texts that aren't cached yet are synthesised in
   play order on a background thread, and a round asks `tts_speak` for
   anything still missing.

3. **`round_submit`** grades one round:
   - It finds the round by index and id. A repeat (double Enter, late
     click) or an unknown session gets a neutral reply and changes nothing.
   - It grades the round's word with FSRS (`deck::grade`). A match round
     grades each pair on its own, so one slip doesn't fail all four.
   - An intro round creates the word's card only now that it has been seen.
     A session abandoned before then leaves no half-met words behind.
   - It scores XP and combo, upserts the session's `events` row and writes
     to `round_log`. XP is kept per round, so quitting mid-session loses
     nothing.
   - Level-ups compare total XP before and after the round.

4. **`session_finish`** checks badges, writes an automatic backup and
   returns the summary.

**Answer checking** for typed answers is in the UI (`src/lib/grading.ts`).
"ñ" is always its own letter, and each round decides whether an answer that
only misses accent marks counts.

### Scheduling and progression

- `deck.rs` uses the [`fsrs`](https://crates.io/crates/fsrs) crate at 90%
  desired retention. FSRS copes with gaps, so time away never builds a
  backlog the player is punished for.
- XP levels (every 500 XP) and the badge list live in
  `crates/tilde-core/src/lib.rs`. Badges are awarded where their condition
  can change: in `score_round` (combos), `round_submit`, `session_finish`
  and `srt_import`.
- Placement asks a few questions and sets the frontier rank. It marks the
  words below it as known.

## Speech

`tts.rs` speaks with Piper and the `es_ES-davefx-medium` voice when
`setup-tts.sh` has installed them, and otherwise with `espeak-ng`.

- The text always goes in on stdin, never as an argument, so text starting
  with "-" can't be read as an option.
- Every run has a 10 s timeout, after which the process is killed.
- After two failures in a row, Piper is skipped for the rest of the run.
- Output is cached as wav files in `audio_cache/`, keyed by engine, voice
  and text. A cache file is written under a temporary name and renamed into
  place, so a killed run never leaves half a file.

Without either engine, rounds simply carry no audio.

## Sentence mining

`srt_import` turns a subtitle file into study material:

1. **Decode.** The UI decodes the file (`src/lib/decode.ts`): UTF-16 with a
   BOM, then strict UTF-8, then windows-1252, which old Spanish `.srt` files
   often use.
2. **Parse.** `tilde_core::srt::parse_subtitles` reads SRT, WebVTT or plain
   text. It drops tags and sound descriptions, and splits a cue into one
   line per speaker turn (a leading dash).
3. **Make sentences.** `lines_to_sentences` joins lines into sentences and
   splits a line that holds more than one. Sentences under three or over
   forty words are dropped.
4. **Match words.** Each sentence is matched against the `FormIndex`. One
   with fewer than 35% of its words in the dictionary is skipped.
5. **Store.** A sentence already mined is skipped, even from another file
   or with other punctuation (`srt::sentence_key`). The rest go into
   `mined` and `mined_words`.

`sentence_add_to_deck` gives each word in a sentence a card.

## Backups

`backup.rs` writes zip files holding a consistent snapshot of the user DB
(`VACUUM INTO`) plus a small JSON description. The file name says what made
a backup, and decides whether it is ever deleted:

| Prefix | Made by | Kept |
| --- | --- | --- |
| `tilde-backup-` | "Exportar ahora" in Settings | always |
| `tilde-auto-` | the end of every session | the newest 10 |
| `tilde-before-reset-` | "Reiniciar progreso" | always |
| `tilde-before-restore-` | a restore | always |

**Where backups go.** They go to `backups/` in the app data directory, or to
a folder the player picks, such as a Syncthing or Nextcloud folder. That
choice is stored in `device.json` next to the database rather than in it,
because it describes this machine and must not travel inside a backup.

**Restoring.** A restore checks the backup before touching anything:

1. The zip must contain a Tilde database.
2. That database must pass `integrity_check` and have the required tables.
3. Its schema must not be newer than the app's. An older one is migrated
   while it is still a separate copy.

Only then does the app back up the current progress, swap the files and
reopen the database. If any step fails, the old progress stays as it was.

**Resetting.** "Reiniciar progreso" backs up the current progress first, then
deletes it in one transaction.

## The UI

| Route | Screen |
| --- | --- |
| `/` | Home: level, sessions to start, badges |
| `/play?kind=` | A session: one component per round type in `lib/components/rounds/` |
| `/sidecar` | Dimmed, keyboard-only drill to run next to a film (no app chrome) |
| `/placement` | The placement quiz |
| `/explore` | Word search and detail, with conjugation tables |
| `/sentences` | Subtitle import and mined sentences |
| `/stats` | Minutes, XP and vocabulary over time |
| `/settings` | Definitions language, session length, sound, backups, credits |

Shared pieces in `src/lib/`:

| File | Does |
| --- | --- |
| `api.ts` | Calls the commands; a call that never answers fails after 30 s |
| `errors.svelte.ts` | The error toast in `+layout.svelte` |
| `keys.ts` | `typingInto()`, so window shortcuts leave text fields alone |
| `components/Modal.svelte` | Dialogs. An open dialog takes all key presses and keeps focus inside |
| `grading.ts` | Answer checking |
| `audio.ts` | Plays round audio |
| `sfx.ts` | Sound effects |
| `confetti.ts` | The celebrations |

The interface text is Spanish.

The CSP in `tauri.conf.json` allows no remote content. Audio is played from
`data:` URLs.

## The content pipeline

`cargo run --release -p tilde-pipeline` rebuilds `content.db` in five steps
(`crates/tilde-pipeline/src/main.rs`):

1. **Sources** (`sources.rs`). These are downloaded once into `data/`:
   - FrequencyWords and Tatoeba, pinned by sha256.
   - The two kaikki.org Wiktionary extractions, which are updated in place
     upstream, so their hashes are recorded in `meta` instead.
2. **Index** (`lexicon.rs`). One pass over English Wiktionary's Spanish
   entries notes which written forms belong to which lemma.
3. **Rank.** The frequency list is lemmatised (*tengo* and *tiene* count
   for *tener*) and the top 12,000 lemmas are kept.
4. **Details.** A second pass collects glosses, inflections and
   conjugation tables for those lemmas:
   - Senses that are Latin-American, vulgar, archaic or slang are passed
     over when there is a plainer one.
   - `curation/glosses.tsv` overrides glosses that read badly out of
     context.
   - Spanish definitions come from Wikcionario (`definiciones.rs`).
5. **Sentences** (`sentences.rs`). Tatoeba pairs whose words are mostly in
   the list are kept and linked to the words they contain.

The database is written under a temporary name and renamed into place.
Rebuilding changes `content.db` in git, so rebuild only when the pipeline or
curation changes, and check what changed before committing it.

## Resilience, in one place

| What goes wrong | What happens |
| --- | --- |
| `content.db` missing or damaged | A native dialog, then the app exits; progress untouched |
| User DB unreadable | Set aside under a new name; the app starts fresh and says where it is |
| User DB from a newer Tilde | Refused, with a message; the file is left as it is |
| A command panics | The lock it held recovers, so later commands still work; the UI shows an error |
| A command never answers | The UI gives up after 30 s and shows an error |
| A speech engine hangs or breaks | Killed after 10 s; Piper is dropped after two failures |
| A bad or foreign backup | Rejected before anything is replaced |
| A crash mid-write | Backups, the content DB and audio files are written under temporary names and renamed |

## Tests and CI

| Command | Covers |
| --- | --- |
| `cargo test --workspace` | Unit tests in each module, command-level tests and smoke tests |
| `npm test` | Answer checking and text decoding |
| `npm run check` | Svelte and TypeScript types |

About the Rust tests:

- The command-level tests are in `src-tauri/src/tests.rs`. They drive the
  real commands through Tauri's mock runtime, against the real `content.db`
  and a temporary user DB, with speech switched off.
- The smoke tests are in `src-tauri/tests/smoke.rs`.
- The pipeline's tests use small inline fixtures and download nothing.

CI (`.github/workflows/ci.yml`) runs all of these on Ubuntu, plus
`cargo clippy -D warnings` and a `rustfmt` check of the pipeline crate.

## Conventions

- **The player's progress is sacred.** Anything that replaces or deletes it
  takes a backup first and validates before it swaps.
- **Schema changes are new migration steps.** Old ones are never edited.
- **Wire types change in two places.** `tilde-core/src/types.rs` and
  `src/lib/contract.ts` must agree.
- **Locks go sessions → user → content.** They are taken with `.locked()`.
- **Nothing slow runs while holding a lock**, if it can be avoided: speech
  is made after the locks are released, or in the background.
- **Comments say why, not what.** Commit messages say what was broken, what
  changed and how it was tested.

### Where common changes go

| Change | Files |
| --- | --- |
| A new round type | Add a `Round` variant in `types.rs` and `contract.ts`, generate it in `session.rs`, grade it in `round_submit`, and render it with a component in `lib/components/rounds/` from `play/+page.svelte` |
| A new badge | `all_badges()` in `tilde-core`, and an `award_badge` call where its condition can change |
| A new setting | `Settings` in `types.rs` and `contract.ts`, `settings()` and `settings_set` in `lib.rs`, and the settings page |
| A schema change | A new step at the end of `db::MIGRATIONS`, and `db::TABLES` if it adds a table |
| A bad gloss | A line in `curation/glosses.tsv`, then a pipeline rebuild |
