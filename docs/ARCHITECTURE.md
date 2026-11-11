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
| `crates/tilde-core/` | Shared types (the wire format), XP levels, badges, the search key, subtitle parsing |
| `crates/tilde-pipeline/` | Builds `content.db` from open data; not shipped |
| `crates/tilde-pipeline/curation/` | Hand-made corrections fed into the pipeline, and the registry of word ids |
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
- `backup_error`: why the latest automatic backup failed, if it did. In
  memory only.
- `app_dir`: the per-user data directory.

Every command is `#[tauri::command(async)]`, so it runs on Tauri's thread
pool. A slow command (speech, a large subtitle file, a backup) never blocks
the window. Two rules keep this safe:

- **Lock order is sessions → user → content.** A command that needs more
  than one lock takes them in that order. `backup_error` is a leaf: it is
  held for a moment, and nothing else is locked while it is. Helpers take
  `&Connection` rather than `&AppState`, because `std::sync::Mutex`
  deadlocks if the same thread locks it twice.
- **Locks are taken with `.locked()`**, which recovers a poisoned mutex. A
  command that panics mid-way (SQLite rolls its statement back) must not
  take every later command down with it.

### Startup

`run()` registers the single-instance plugin first: a second launch brings
the window of the Tilde already running forward and ends there. Setup then
resolves the app data directory, and:

1. Takes `tilde.lock` in that directory (`lock_instance`). The plugin works
   over the session bus; the lock file is the backstop where there is none.
   If another Tilde holds it, a native dialog says so and the app exits.
2. Opens `content.db`. If it is missing or unreadable, a native dialog
   explains that reinstalling fixes it, and the app exits.
3. Opens `tilde_user.db` with `open_progress`. `db::open_or_set_aside`
   checks the whole file (`PRAGMA quick_check`) before it migrates it. A
   damaged file is renamed to `tilde_user.db.damaged-<time>`, a fresh
   database is started in its place, and `backup::recover_newest` fills
   that from the newest backup that restores. A dialog says where the old
   file is and the date of the backup recovered, so nothing is deleted.
   - A file is set aside only on proof of damage. One from a newer Tilde is
     refused with a message and left as it is; a check that couldn't run
     proves nothing.
   - Recovery recovers nothing if the newest backup is one made before a
     reset or a restore. The player's last act was deliberate, and undoing
     it is theirs to choose in Settings; they start fresh and are told so.
4. Discovers the speech engines (`Tts::discover`).
5. In the background, deletes the temporary files a killed run left behind
   (`backup::sweep_stale`): in the data directory, the audio cache and the
   backup folder, by exact name, and only those over an hour old.

### Commands

The UI sees only these commands, all in `lib.rs` and wrapped in
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
`src/lib/contract.ts`. Two tests read `contract.ts` and fail if the two
drift apart: `round_wire_format_matches_contract_ts` for the `Round`
variants, and `stats_word_and_sentence_wire_formats_match_contract_ts` for
`WordHit`, `SentencePage`, `StatsData`, `BackupProblem` and `BackupFolder`.

Where there is nothing to say yet, the wire says `null` and not a zero that
would read as an answer: `StatsData.accuracyAllTime` is null until a round
has been answered.

A command that can fail to save returns a `Result`. Its error is a sentence
in Spanish for the player, which the UI shows as it is: `db::user_error`
words a failed write to the progress, and `backup.rs` a failed backup.
`session_finish` is the exception and always returns its summary: what the
session earned was saved round by round.

## Data

### `content.db`: what there is to learn

Read-only, and opened with SQLite's `immutable=1` (`sqlite_file_uri` writes
the path as a URI, a Windows path included). It is built by the pipeline,
committed to the repository and bundled as a Tauri resource. Its schema
version (`version` in `meta`) is 3.

| Table | Holds |
| --- | --- |
| `words` | One row per lemma: its `id`, its frequency `rank` (1 = most common), `lemma_key` (the lemma as a search is matched against it), `pos`, `gloss_en`, `gloss_es`, and the flags `region` (`latam`: used only outside Spain) and `register` (`vulgar`) |
| `forms` | Every written form and the lemma it belongs to, with a tag (`plural`, `participio`, `tense\|person`) |
| `form_keys` | The search key of every form that has an accent or an ñ, so that a search typed without them finds its word |
| `conjugations` | Verb tables from Wiktionary: `(word_id, tense, person) → form` |
| `sentences`, `sentence_links` | Tatoeba es–en pairs and the words each one is an example of. A link records the token that is the word (`form`) and whether the English shows the word's gloss (`quality` 0 when it does) |
| `meta` | Schema version, build time, counts, attribution text and the sha256 of each source |

**Word ids.** A word's `id` is not its rank. It is the same in every build
of the database (see [the pipeline](#the-content-pipeline)), because the
player's cards are keyed by it. A rebuild can still drop a word. Its card
then stays in the user database, untouched, and works again if the word
returns; meanwhile it is neither served, counted nor reset. `ContentDb`
reads the set of ids once (`word_ids`), and whatever feeds sessions and
numbers in `deck.rs` takes that set and filters by it.

`content.rs` wraps the queries:

- **Search** (`search`) folds the query with `tilde_core::search_key`
  (lowercase, no accents, ñ as n) and looks for it in `lemma_key`, in
  `forms` and `form_keys`, and in the English gloss. Results come in tiers:
  the word as typed, the word without its accents, a word the query is a
  form of, a word that starts with it, a gloss that has it as a whole word,
  then anything that merely contains it. Within a tier the commoner word
  comes first, except that among the words the query is a form of, those
  that have it as typed come before those that have it with other accents.
- **What may be taught.** One SQL condition, `TEACHABLE`, says which words
  may be introduced, placed by or offered as wrong answers: those with a
  gloss that are flagged neither vulgar nor Latin-American. `new_candidates`
  gives the commonest of them that the player has no card for.
- **Wrong answers.** `distractor_candidates` gives teachable words with no
  meaning in common with the answer, its own part of speech first, the
  nearest in frequency first. `similar_words` keeps to the answer's part of
  speech, for the cloze.
- **Example sentences** (`sentences_for_word`) are those of 15 to 140
  characters. Each comes with the token that is the word, and those whose
  English shows the word's gloss come first.
- **`FormIndex`** maps a written form back to one word, for sentence
  mining. A form that is itself a headword is that word. Otherwise it goes
  to a teachable word that has it in its tables, and not only as an
  imperative; among those, to the commonest. An exact spelling wins over
  the accent-free fallback. The index is built on each call (about 0.2 s in
  a release build) rather than kept in memory.

### `tilde_user.db`: the player's progress

Lives in the app data directory, in WAL mode with foreign keys on. All
progress is in this one file, which is why backups are simple.

| Table | Holds |
| --- | --- |
| `cards` | FSRS state per word the player has met: stability, difficulty, due time, reps, lapses |
| `reviews` | Every grade given, for history and stats |
| `events` | One row per session: XP, seconds played, kind. Total XP is `SUM(xp)` |
| `round_log` | One row per round submitted, for per-skill badges and stats |
| `badges` | Earned badges and when |
| `mined`, `mined_words` | Sentences imported from subtitles, and the words each was matched to at import. `mined_words` is a record: nothing reads it back |
| `settings` | Key/value: definition language, session length, sound, the placement result and the rank it cut at |

**Migrations.** `db::MIGRATIONS` is an append-only list of SQL steps.
`PRAGMA user_version` records how many have run. Today there are two: the
schema, and a data fix that takes "Termina tu primera sesión" back from a
player who was given it for the placement quiz alone. `db::migrate` runs
the steps a database has not had in one transaction, and refuses a database
from a newer Tilde rather than guessing. To change the schema, append a
step: never edit or reorder an old one.

**Writes.** A command that writes more than one row runs its writes
through `db::in_transaction`: one `IMMEDIATE` transaction, so all of them
are saved or, on any error, none. While it runs, nothing else can be
committed on the connection. A full disk or an I/O error makes SQLite end
the transaction by itself, and without that guard whatever the command
wrote next would be saved on its own. So nothing that commits by itself
(`VACUUM`, a nested `BEGIN`) may run inside it. `session_finish` is the
one exception: the session's row and each badge are writes of their own. A
badge that can't be written is not earned yet, and is checked for again
when the next session ends.

## How a session works

1. **`session_start`** calls `session::generate`, which builds every round
   up front from the deck and the content DB:
   - `gen_mixed` (Quick, Standard, Deep) mixes due reviews, new words (each
     an intro card followed by a first quiz), games, conjugation drills and
     match rounds. The intro blocks come first; the rest is shuffled.
   - `gen_reviews` (Solo repaso) serves only due cards, as review cards.
   - `gen_sidecar` serves keyboard-only choice, listening and dictation
     rounds on words that have a card. With no cards it has no rounds, and
     the page says to play a session first.

   It also drops the sessions nobody has answered for six hours
   (`session::STALE_S`): ones whose page went away without finishing them.

2. **Audio** in generated rounds is cache-only, so generation never waits
   for a speech engine. Texts that aren't cached yet are synthesised in
   play order on a background thread, and a round asks `tts_speak` for
   anything still missing.

3. **`round_submit`** grades one round:
   - It finds the round by index and id. A repeat (double Enter, late
     click) or an unknown session gets a neutral reply and changes nothing.
   - Everything it writes is one transaction (`grade_round`). If that
     fails, nothing is saved, the session in memory is put back as it was
     (`Session::mark` and `rewind`), and the round can be answered again.
   - It grades the round's word with FSRS (`deck::grade`): right or wrong,
     or on a review card the grade the player gave (`RoundResult.quality`).
     A match round grades each pair on its own, so one slip doesn't fail
     all four.
   - An intro round creates the word's card only now that it has been seen.
     A session abandoned before then leaves no half-met words behind. If
     the player answers the card with "Ya me la sé" (`quality` 4), the word
     is saved as known instead (`deck::mark_known`).
   - An intro card is shown, not answered: it counts in neither the
     session's rounds, its accuracy nor the combo, and earns no XP.
   - The session's length is the time spent on its rounds: each round
     reports its `duration_ms`, and at most two minutes of it count
     (`session::ROUND_MS_CAP`), so a window left open is not play.
   - It scores XP and combo, upserts the session's `events` row and writes
     to `round_log`. XP is kept per round, so quitting mid-session loses
     nothing.
   - Level-ups compare total XP before and after the round.

4. **`session_finish`** checks badges and returns the summary. If the
   player got through at least one round, an intro card included, it also
   writes an automatic backup. The UI calls it when the last round is
   answered, when the player leaves, and at once for a session that
   arrives with no rounds.

**Answer checking** for typed answers is in the UI (`src/lib/grading.ts`).
"ñ" is always its own letter, and each round decides whether an answer that
only misses accent marks counts. `graded_as` in `session.rs` is the Rust
twin of its `norm`, used to deal build tiles: a change to one is a change
to both.

### What gets taught, and when

- **New words** are the commonest teachable words that have no card yet,
  whatever the player's level: starting from the commonest each time leaves
  no gap between the words a player knows and the ones they are taught.
- **Nothing unseen is quizzed.** Games are played with the words that have
  a card and the ones this session introduces, and no others. While the
  deck is too small to fill the games, the session introduces more words
  instead, at most twice the usual number, and is otherwise shorter.
  `session::planned_new_words` is that number; Home shows it too.
- **"Known"** has one definition, `deck::KNOWN`: the card is in review and
  FSRS expects the word to be remembered three weeks from now. Home, the
  level estimate, Explore and the stats all use it. `deck::mark_known`
  (placement, "Marcar como conocida", "Ya me la sé") only ever strengthens
  a card.

### What a round promises

- **One option builder.** Every round that offers words or meanings to
  pick from, the placement questions included, gets its options from
  `options_for`: four options, no two that read the same, and none that is
  right as well. No wrong answer shares a meaning with the word; where the
  word is only heard, none sounds like it or like another option.
- **One language per round.** With "Español primero" a round shows Spanish
  definitions if its word has one, and is otherwise in English throughout
  (`meaning_lang`). It never mixes the two.
- **Match** rounds are groups of four words, no two of which share a
  meaning (`match_chunks`). A word with no meaning in the round's language
  is left out.
- **Cloze** rounds need an English translation, which is shown with the
  gap. The gap is the token the example is linked by. The other two or
  three options are similar words in the same form (`similar_words`),
  without any the translation names.
- **Build** tiles are never dealt in an order that is already graded as
  right (`build_tiles`).
- **Dictation** (hear a sentence, write it) takes a sentence of at most 40
  characters that the session has not used yet. In a normal session it is
  only for a word met before this session, once or twice a session. In the
  sidecar, where every word has a card, every third round may be one.
- **Without a speech engine** no listening round and no dictation is
  generated in any session: the word is quizzed in writing instead.

### Scheduling and progression

- `deck.rs` uses the [`fsrs`](https://crates.io/crates/fsrs) crate at 90%
  desired retention. FSRS copes with gaps, so time away never builds a
  backlog the player is punished for.
- XP levels (every 500 XP) and the badge list live in
  `crates/tilde-core/src/lib.rs`. Badges are awarded where their condition
  can change: in `score_round` (combos), `session_finish` and `srt_import`.
  - "Termina tu primera sesión" takes a finished session with at least one
    answer in it; intro cards alone are not one.
  - The "Aprende N palabras" badges count known words the player has
    answered at least once (`deck::learned_count`). Placement and "Marcar
    como conocida" say a word is known; they don't make it one learned here.
- **Placement** asks one word from each of ten frequency bands, in English
  (`placement_items`), and runs once: `placement_submit` asked again
  returns the stored result.
  - Right answers count up to the second miss, and the cut is the rank of
    the second-hardest of them, at most 1800 (`placement_cut`). With four
    options a guess is right one time in four, so where the right answers
    fall says more than how many there are.
  - Every teachable word commoner than the cut that has no card is marked
    as known. Fewer than two right answers mark nothing.
  - "No lo sé" is sent as a miss. "Empiezo de cero" skips the questions and
    submits no answers, so nothing is marked.

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

Without either engine, rounds carry no audio, and the rounds that need it
are not generated (see above).

## Sentence mining

`srt_import` turns a subtitle file into study material:

1. **Decode.** The UI decodes the file (`src/lib/decode.ts`): UTF-16 with a
   BOM, then strict UTF-8, then windows-1252, which old Spanish `.srt` files
   often use.
2. **Parse.** `tilde_core::srt::parse_subtitles` reads SRT, WebVTT or plain
   text. It normalises the text to NFC first, so a decomposed file (as
   macOS tools write them) reads like any other. It drops tags and sound
   descriptions, and splits a cue into one line per speaker turn (a leading
   dash).
3. **Make sentences.** `lines_to_sentences` joins lines into sentences and
   splits a line that holds more than one. A sentence that trails off with
   "..." and is picked up by the next subtitle stays whole, and dots that
   only mark the continuation are dropped. Sentences under three or over
   forty words are dropped.
4. **Match words.** Each sentence is matched against the `FormIndex`, where
   a written form leads to one word. Stopwords don't count. A spelling the
   dictionary knows is judged as written, so "está", "sé" and "té" are
   words while "esta", "se" and "te" are stopwords. A sentence with nothing
   to learn in it (only stopwords or numbers), or with fewer than 35% of
   its words in the dictionary, is skipped.
5. **Store.** A sentence already mined is skipped, even from another file
   or with other punctuation: `srt::sentence_key` compares its words and
   numbers, lowercased and without accents. The rest go into `mined` and
   `mined_words`, the whole file in one transaction.

`sentences_list` serves the mined sentences a page at a time, newest first
(`before_id`, `limit`), with their total (`SentencePage`).

`sentence_add_to_deck` matches the sentence again, as an import would match
it today, and gives each word found a card. It does not read `mined_words`,
which may hold what an older dictionary matched.

## Backups

`backup.rs` writes zip files holding a consistent snapshot of the user DB
(`VACUUM INTO`) plus a small JSON description. The file name says what made
a backup, and decides whether it is ever deleted:

| Prefix | Made by | Kept |
| --- | --- | --- |
| `tilde-backup-` | "Exportar ahora" in Settings | always |
| `tilde-auto-` | the end of every session played, and choosing a backup folder | the newest 10 |
| `tilde-before-reset-` | "Reiniciar progreso" | always |
| `tilde-before-restore-` | a restore | always |

**Where backups go.** They go to `backups/` in the app data directory, or to
a folder the player picks, such as a Syncthing or Nextcloud folder. That
choice is stored in `device.json` next to the database rather than in it,
because it describes this machine and must not travel inside a backup.

**When one fails.** Nobody is waiting for an automatic backup, so a failed
one interrupts nothing. The reason is kept in `backup_error` and Settings
shows it (`BackupFolder.lastError`) until a backup into the folder works
again. A damaged database is never written as a backup: `VACUUM INTO` reads
every row and refuses it. The one exception is the copy taken before a
restore, which is how a player gets out of a damaged database: what is
being replaced is kept page by page, marked `"damaged": true` in its
description.

**Restoring.** A restore checks the backup before touching anything:

1. The zip must contain a Tilde database.
2. That database must pass `integrity_check` and have the required tables.
3. Its schema must not be newer than the app's. An older one is migrated
   while it is still a separate copy.

Only then does the app back up the current progress and copy the checked
database into the open one through SQLite's backup API, as one transaction.
The connection and the file stay the same ones. If any step fails, the old
progress stays as it was, and the safety copy just written is removed
again. Sessions in progress are dropped first: their rounds belong to the
progress being replaced.

**Resetting.** "Reiniciar progreso" backs up the current progress first, then
deletes it in one transaction.

## The UI

| Route | Screen |
| --- | --- |
| `/` | Home: level, sessions to start, badges |
| `/play?kind=` | A session. `+page.svelte` gives every session a `PlaySession.svelte` of its own, with one component per round type in `lib/components/rounds/` |
| `/sidecar` | "Modo secundario": a dimmed, keyboard-only quiz on words already met, to run next to a film. A page of the main window without the app chrome, not a window of its own |
| `/placement` | The placement quiz, which the player can skip |
| `/explore` | Word search and detail, with conjugation tables. A word is marked as known, as being learned (it has a card: `WordHit.inDeck`) or as new |
| `/sentences` | Subtitle import and mined sentences |
| `/stats` | Minutes, XP and vocabulary over time |
| `/settings` | Definitions language, session length, sound, backups, credits |

Shared pieces in `src/lib/`:

| File | Does |
| --- | --- |
| `api.ts` | Calls the commands; a call that never answers fails after 30 s |
| `sessionFlow.ts` | One session from start to summary, with no framework in it: `createFlow` sends each answer at once, holds the reveal, moves on, pauses under the exit dialog and finishes the session exactly once. `/play` and `/sidecar` both run on it |
| `celebrate.svelte.ts`, `badgeQueue.ts` | The badge toasts, which the layout shows on every page: at most three at a time, four seconds each, one sound for a batch |
| `errors.svelte.ts` | The error toast in `+layout.svelte` |
| `keys.ts` | `typingInto()`, so window shortcuts leave text fields alone; `activates()`, for Enter or Space as a screen's main action; `optionFromKey()` and `choiceFromKey()`, for what a digit key picks |
| `components/Modal.svelte` | Dialogs. An open dialog takes all key presses and keeps focus inside |
| `grading.ts` | Answer checking |
| `defs.ts` | Which meaning of a word is shown under the definitions setting, the Spanish names of the parts of speech, and smaller type for long texts |
| `audio.ts`, `clipPlayer.ts` | Plays round audio: one clip at a time, and none that arrives after the player has moved on |
| `sfx.ts` | Sound effects, muted until the saved setting has been read |
| `confetti.ts` | The celebrations |
| `format.ts` | Spanish wording: plurals, due times, a session's minutes, the accuracy ring |
| `chart.ts` | Where the stats charts put their points and labels |
| `motion.ts` | Transition durations that respect reduced motion |
| `wordCard.ts` | Which word's card Explore shows when answers come back out of order |
| `deal.ts` | How a match round's columns are dealt: never both in the same order |
| `placement.ts` | What the placement quiz sends for an answer, for "No lo sé" and for "Empiezo de cero" |
| `overflow.ts` | Whether a box that scrolls inside the page has more below; the session summary packs itself tighter by it |

Most of these have no runtime imports (types only), so that `node --test`
can load them as they are. Logic worth testing goes in such a module and
the Svelte file stays thin. Where logic needs Svelte or the commands, the
testable part is split off: `celebrate.svelte.ts` wraps `badgeQueue.ts`,
`audio.ts` wraps `clipPlayer.ts`.

Around the pages:

- **Sound** follows the saved setting and nothing else. `+layout.svelte`
  reads the settings when the app starts and switches `sfx.ts` on from
  them; the setting is kept in the user database only.
- **The nav** has its width in one place, `--nav-w` in `src/app.css`, which
  the page, the feedback pill and the badge toasts all read. The bar beside
  the active link is one element that slides between the links, placed by
  measuring the link.
- **Small windows.** The app opens at 1160×780 and allows 940×620. `short:`
  is a Tailwind variant (`src/app.css`) for a window under 720 px high, and
  `tight:` one for a box marked `data-tight`; a screen that has to fit
  without scrolling packs tighter with them.

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
   for *tener*) and the 12,000 commonest lemmas that get a gloss are kept.
   Three curation lists steer the count: a form in `curation/exclude.txt`
   counts for nothing, a form in `curation/inflections.txt` counts for the
   word it inflects, and a form in `curation/lemmas.txt` stays a word of
   its own.
4. **Details.** A second pass collects glosses, inflections and
   conjugation tables for those lemmas:
   - A gloss is up to three senses within 48 characters. Senses that are
     Latin-American, vulgar, archaic or slang are passed over when there is
     a plainer one. A sense that names Spain is Peninsular, whatever other
     regions it names.
   - A sense that only repeats the word (*hotel*: "hotel") is its gloss
     when it leads the entry: a cognate is its own translation. A noun or
     adjective with no sense of its own takes the meaning of the word it
     points to (*moto*, a clipping of *motocicleta*).
   - `curation/glosses.tsv` overrides glosses that read badly out of
     context, and the part of speech where its third column gives one. Only
     a word whose part of speech is verb gets a conjugation table.
   - A word is flagged `vulgar` when every sense of it is, when the gloss
     it would be taught by comes from a vulgar sense, or when
     `curation/vulgar.txt` lists it. It is flagged `latam` when every sense
     is labelled as used outside Spain, unless
     `curation/not_regional.txt` lists it.
   - A noun, pronoun or article loses the forms of the other gender when
     that counterpart is a word of its own: *padre* is no form of *madre*.
   - Spanish definitions come from Wikcionario (`definiciones.rs`): the
     first current Peninsular sense of the word's part of speech, without
     wiki markup. That sense is often not the everyday one, and Wikcionario
     changes every week, so the commonest words take their definition from
     `curation/definiciones.tsv` instead.
5. **Sentences** (`sentences.rs`). Tatoeba pairs whose words are mostly in
   the list are kept and linked to the words they show. Pairs listed in
   `curation/bad_sentences.txt` are left out, and so are pairs written with
   *vos* forms, which are not Castilian.
   - A token is a run of letters. Letters that touch a digit are none: the
     *CO* of "CO2" is no example of *co*.
   - A token that is a word's own headword is that word, unless the English
     shows another reading and not that one: *vino* with "came" is *venir*.
   - A token that is nobody's headword goes to the readings the English
     shows, or to all of them when it shows none.
   - The English shows a reading when it has every word of one sense of
     that word's gloss, in any form: regular forms are known by their
     endings, the others are listed in `curation/english_forms.tsv`.
   - Each link stores its token (`form`) and whether its word is shown
     (`quality`).

**Word ids.** The app keys progress by `words.id`, so an id has to mean the
same word in every build. `curation/word_ids.tsv` records the id of every
lemma ever selected (`ids.rs`): a build looks its lemmas up there, only new
lemmas get new ids, and no line is ever removed.

- A build to the default path rewrites the registry. Commit it together
  with the database.
- A build with `--out <path>` is a trial: it reads the registry and leaves
  it alone, so it can be run freely.
- Renaming a headword without losing its id is a hand edit of the registry;
  the file's header says how.
- The test `committed_content_matches_the_word_id_registry` fails when the
  committed database and the registry disagree.

**The build report.** The build prints what a person should look at:
curated definitions of words no longer in the list, blocklisted sentences
Tatoeba no longer has, the commonest *vos* tokens (a name among them
belongs in `curation/exclude.txt`), and the commonest words left without a
usable example. It ends with what changed against the committed database
(`ids::report`): the lemmas that entered and left, pairs that look like one
word under a new headword, the glosses that changed among the 5,000
commonest words, and an `ERROR` if any id would change.

The database is written under a temporary name and renamed into place.
Rebuilding changes `content.db` in git, so rebuild only when the pipeline or
curation changes, and check what changed before committing it.

## Resilience, in one place

| What goes wrong | What happens |
| --- | --- |
| `content.db` missing or damaged | A native dialog, then the app exits; progress untouched |
| User DB damaged, anywhere in the file | Set aside under a new name; the newest backup is recovered in its place, and a dialog says where the old file is |
| User DB from a newer Tilde | Refused, with a message; the file is left as it is |
| Tilde is started twice | The window already open comes forward; failing that, the lock file refuses the second one with a dialog |
| A write fails (full disk, busy file, damage) | Nothing of the command is saved, the player is told why in plain words, and the round or action can be tried again. `session_finish` is the exception: its summary always comes |
| An automatic backup fails | Play goes on; Settings shows why until a backup works |
| A word leaves the dictionary | Its card is kept but neither served nor counted, and works again if the word returns |
| A command panics | The lock it held recovers, so later commands still work; the UI shows an error |
| A command never answers | The UI gives up after 30 s and shows an error |
| A speech engine hangs or breaks | Killed after 10 s; Piper is dropped after two failures |
| No speech engine at all | Rounds carry no audio, and listening rounds become written ones |
| A session is never finished | What it earned was saved round by round; it is dropped from memory once it is six hours stale |
| A bad or foreign backup | Rejected before anything is replaced |
| A crash mid-write | Backups, the content DB and audio files are written under temporary names and renamed; what a killed run left behind is swept at the next start |

## Tests and CI

| Command | Covers |
| --- | --- |
| `cargo test --workspace` | Unit tests in each module, command-level tests and smoke tests |
| `npm test` | The modules of `src/lib` that run without a browser: the session flow, answer checking, key handling, the toast queue, the clip player, text decoding, wording, text sizes, chart geometry, the match deal, placement answers, Explore's card, the overflow watch, the sound switch, and source checks of `src/app.css` |
| `npm run check` | Svelte and TypeScript types |

About the Rust tests:

- The command-level tests are in `src-tauri/src/tests.rs`. They drive the
  real commands through Tauri's mock runtime, against the real `content.db`
  and a temporary user DB, with speech switched off or a stand-in engine.
- The smoke tests are in `src-tauri/tests/smoke.rs`.
- The pipeline's tests use small inline fixtures and download nothing. They
  also read the real curation files, so a malformed row fails a test before
  it fails a build.

CI (`.github/workflows/ci.yml`) runs all of these on Ubuntu, plus a build
of the UI, `cargo clippy -D warnings` and a `rustfmt` check of the pipeline
crate.

## Conventions

- **The player's progress is sacred.** Anything that replaces or deletes it
  takes a backup first and validates before it replaces.
- **Schema changes are new migration steps.** Old ones are never edited.
- **A command that writes more than one row does it in one transaction**
  (`db::in_transaction`), `session_finish` aside, and a write that fails is
  reported in the player's language (`db::user_error`).
- **A word's id never changes.** `curation/word_ids.tsv` is committed with
  the database it was written for.
- **Wire types change in two places.** `tilde-core/src/types.rs` and
  `src/lib/contract.ts` must agree.
- **Locks go sessions → user → content.** They are taken with `.locked()`.
- **Nothing slow runs while holding a lock**, if it can be avoided: speech
  is made after the locks are released, or in the background.
- **A fix for one word goes in a curation file, not in code.** Code holds
  rules; the lists hold the exceptions.
- **Only the pipeline crate is formatted with `rustfmt`**, and CI checks
  it. The other two crates are not: don't reformat them.
- **Comments say why, not what.** Commit messages say what was broken, what
  changed and how it was tested.

### Where common changes go

| Change | Files |
| --- | --- |
| A new round type | Add a `Round` variant in `types.rs` and `contract.ts`, generate it in `session.rs` (its options from `options_for`), grade it in `grade_round`, render it with a component in `lib/components/rounds/` from `play/PlaySession.svelte`, and give it a reveal time in `PLAY_REVEAL_MS` (`sessionFlow.ts`) |
| A new badge | `all_badges()` in `tilde-core`, and an `award_badge` call where its condition can change (with `?` when inside a transaction) |
| A new setting | `Settings` in `types.rs` and `contract.ts`, `settings()` and `settings_set` in `lib.rs`, and the settings page. One that must hold from the first screen on, as sound does, is also read in `+layout.svelte` |
| A schema change | A new step at the end of `db::MIGRATIONS`; for a new table also `db::TABLES` and the list `progress_reset` empties |
| Something more that grading changes in a `Session` | The field, and `SessionMark` with it, so a failed save can put it back |
| A bad English gloss, or a wrong part of speech | A row in `curation/glosses.tsv`: lemma, gloss of at most 48 characters, and the part of speech as an optional third column |
| A wrong Spanish definition | A row in `curation/definiciones.tsv`: lemma and definition, or `-` for none |
| A form taught as a word of its own (*nueva*, *pienso*) | A line in `curation/inflections.txt`, the forms that are no words of their own. Its count goes to the word it inflects, and so does the count of whatever is a form of it |
| A form that should be a word of its own (*muy*, *fuera*) | A line in `curation/lemmas.txt` |
| A name, an English word or a misspelling in the list | A line in `curation/exclude.txt`. Such a form counts for nothing, is never a word, and is stored as a form of none |
| A vulgar word that isn't flagged | A line in `curation/vulgar.txt` |
| A word wrongly flagged Latin-American | A line in `curation/not_regional.txt` |
| A bad example sentence | A line in `curation/bad_sentences.txt` |
| An example under the wrong word because an English form wasn't recognised | A row in `curation/english_forms.tsv` |
| A headword that should be spelled differently | A hand edit of `curation/word_ids.tsv`, as its header says |

Every curation file starts with a comment that says what belongs in it and
how a line is written. A change to any of them takes a pipeline rebuild.
