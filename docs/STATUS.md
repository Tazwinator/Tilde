# Where Tilde stands, and what is left

Written on 2026-11-11, at commit `e7ea3f8`, after a full review of the app
turned up 41 confirmed problems and all of them were fixed. This file is for
picking the work up again: what was decided and why, what was never checked
in the real app, what is known to be imperfect, and how to go about the
usual jobs. How the code works is in [ARCHITECTURE.md](ARCHITECTURE.md);
this file does not repeat it.

## What the round of fixes covered

| Area | What changed |
|---|---|
| Word ids | A word keeps its id in every build (`curation/word_ids.tsv`), so a player's cards stay on their words when the word list changes |
| What gets taught, and when | New words in order of frequency; no quizzing of words never shown; a placement quiz that runs once; "Ya me la sé"; one definition of "known" |
| Rounds | One option builder for every multiple-choice round; cloze blanks the linked token and shows the translation; match and build rounds graded fairly |
| English glosses, flags, junk | Rules for Peninsular senses, cognates, pointer senses; vulgar and Latin-American flags; names, English and stray forms out of the list |
| Spanish definitions | Everyday sense first; the 3,000 commonest words reviewed by hand in `curation/definiciones.tsv` |
| Example sentences | Linked to the word they show, with the token and whether the English shows the meaning; *vos* sentences dropped |
| Play screens | One session controller (`sessionFlow.ts`), keyboard play, badges, summary |
| Other screens | Search by form and without accents, stats, Frases, settings |
| Storage and safety | Transactions, automatic backups, recovery from a damaged progress file, one running instance |
| Subtitles | `.srt`, `.vtt` and plain text; cue clean-up; sentences rejoined across cues |

## Decisions, and the reason for each

These were judgement calls. Change them if you disagree, but know why they
are as they are.

- **Placement is cautious.** Right answers count only up to the second
  miss, and the cut is just past the second-hardest of them, never beyond
  rank 1800. A word wrongly taken as known is never taught; one wrongly
  left costs a tap on "Ya me la sé".
- **"Ya me la sé" still gets one check.** The word is filed as known at
  once, but the quiz round already planned for it in that session still
  comes. A right answer keeps it known; a wrong one lowers it.
- **Known words are practised too.** Games draw on the whole deck, words
  from placement included, and a miss lowers the word's strength.
- **"Known" takes time.** A word counts as known at state 2 with stability
  of 21 days or more, so Inicio can say "0 palabras" after a first session.
  Badges count words answered at least once instead.
- **The first-session badge needs one answered question.** A session of
  intro cards alone is saved and backed up, but earns no badge.
- **A cloze may have two grammatical answers.** The English translation is
  shown with the gap and settles which one is meant.
- **Sentence mining maps each written form to one word.** A card for every
  word a form could be would teach *crear* to someone whose film said
  *creo*. The cost: "Vino ayer" is filed under wine, "ven aquí" under *ver*.
- **Spanish definitions are stored as text**, not as a sense number,
  because the Wikcionario download changes every week. About two in five
  of the reviewed ones were written for Tilde.
- **An example "shows" a word only when the English has every word of one
  sense of its gloss.** Looser matching confirmed wrong readings.
- **No automatic recovery after a deliberate restore or reset.** If the
  newest backup is the one made before a restore or a reset, a damaged
  progress file is set aside and the player starts fresh and is told to
  restore by hand. Recovering would undo what they had just chosen.
- **Mostly-capitals subtitle lines are dropped** as captions and signs.
- **Not built:** a floating always-on-top window for Modo secundario; a
  Windows CI job (Windows is not a target yet); deleting, searching or
  word-by-word display of mined sentences in Frases; a streak counter.

## How it was verified

- At `e7ea3f8` a clean checkout passes `cargo test --workspace` (150 app,
  4 smoke, 28 core and 87 pipeline tests), `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo fmt -p tilde-pipeline --check`,
  `npm run check` and `npm test` (124).
- Three testers played the built app from fresh profiles: first run and
  learning; rounds in "Español primero" and Explorar; Frases, statistics,
  settings, backups, Modo secundario and a 940x620 window. What they found
  and was real is fixed (commits `524b3a6` to `e7ea3f8`).

### Never checked in the real app

- **Sound.** The test machine had no sound card. Listening and dictation
  rounds were never heard; whether the audio matches the word is untested.
- **Some rounds were barely met** in the last pass: verb conjugation, match
  pairs, grading a review card, and playing build-a-sentence and match
  pairs with the keyboard alone.
- **The last three fixes** (no two wrong answers that say the same, the
  placement cut, the gloss shortener) are covered by tests; only the
  Explorar hint and one corrected gloss were looked at on screen.
- **Windows and macOS.** Everything was built and run on Linux.

## Known to be imperfect

- *inspector* is the one word among the 2,000 commonest without an example
  sentence: Tatoeba has no pair with it. A hand-written example would need
  a way to add sentences that the pipeline does not have yet.
- 75 of the 5,000 commonest words have no usable example.
- Searching for "ñ" alone lists words that start with "n", because the
  search folds ñ to n. Typing "ESTÁ" lists *esta* before *estar*.
- In "Español primero", two definitions can both sound right when one is
  generic (*que* is "Nexo gramatical para unir una oración con otra").
- 17 words flagged vulgar among the 3,000 commonest were left out of the
  hand review of glosses and definitions.
- Glosses past rank 3,000 come straight from the rules, apart from 73
  fixed by hand. Expect more that miss the everyday word.
- `round_log.session_id` restarts with the app, so it does not identify a
  session across runs. Nothing reads it.
- The progress dots in the placement quiz and Modo secundario turn green
  for every answered question, right or wrong.
- A long lemma in an Explorar row squeezes its gloss down to a letter or
  two and "…".
- A file that is not subtitles is reported as "nothing new", not as "this
  is not a subtitle file".
- The word database is a 44 MB file committed whole on every rebuild.
  Git LFS, or a release asset, would stop the repository growing.

## Finding data problems

The word data is where most remaining problems are. These checks found
nearly all of the ones fixed at the end, and are worth running after any
rebuild (python with `sqlite3`, on `src-tauri/resources/content.db`).

1. **Glosses that miss the everyday word.** For each teachable word with
   six or more rows in `sentence_links`, the share with `quality = 0`.
   Under 15 in 100 means the English translations rarely use the gloss:
   read those glosses. 68 of the 3,000 commonest are left, most of them
   function words and words bound to a phrase.
2. **The first example of each common word.** It is the one on the card
   when the word is taught: `ORDER BY l.quality, l.sentence_id LIMIT 1`
   over sentences of 15 to 140 characters. Read the first few hundred.
3. **Teachable words with neither a Spanish definition nor an example**
   among the commonest 6,000: mostly English, names and fragments for
   `curation/exclude.txt`.
4. **Glosses ending in "…".** The pipeline could not shorten them to a
   meaning; each wants a row in `curation/glosses.tsv`. There are none now.
5. **The build report**, which lists what entered and left the list and
   which glosses changed among the 5,000 commonest.

Which curation file fixes which kind of problem is the table at the end of
ARCHITECTURE.md. Try a change with `cargo run --release -p tilde-pipeline
-- --out /tmp/content.db` (a trial build, about eight minutes, touches
nothing) before the real build.

## Testing the real app without a screen

The app runs under a virtual display, driven with `xdotool` and looked at
through screenshots. This is how the live checks were done.

```sh
npm run build
cargo build -p tilde --features tauri/custom-protocol   # the UI is embedded
W=/tmp/tilde-test; mkdir -p $W/home
Xvfb :91 -screen 0 1280x860x24 & echo $! > $W/xvfb.pid
( cd src-tauri && HOME=$W/home DISPLAY=:91 WEBKIT_DISABLE_DMABUF_RENDERER=1 \
  WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1 \
  exec ../target/debug/tilde > $W/app.log 2>&1 ) & echo $! > $W/app.pid
DISPLAY=:91 import -window root $W/shot.png      # a screenshot
DISPLAY=:91 xdotool mousemove 100 247 click 1    # a click (Explorar)
DISPLAY=:91 xdotool type --delay 80 'tenia'      # typing
```

- The app finds `resources/content.db` relative to the directory it is
  started in, hence the `cd src-tauri`.
- `HOME` gives the run its own profile. Progress is then in
  `$W/home/.local/share/com.tazwinator.tilde/tilde_user.db`, backups in
  `backups/` beside it.
- A fresh profile opens on the placement quiz. Keys 1 to 4 pick options.
- Stop it with `kill $(cat $W/app.pid)`, never by name.

## Running it somewhere other than a desktop

Tilde is a desktop app, not a web app. The screens are web technology
(Svelte), but they run inside the app's own window and reach the Rust side
through Tauri's in-process calls, not over HTTP. There is no server to put
in a container as it stands. Two ways to get there:

1. **The desktop app in a container, shown in a browser** (a virtual
   display with KasmVNC or noVNC). No code changes. It is a remote desktop:
   one user, some lag, and sound needs a stream of its own, which matters
   because listening rounds are a large part of a session.
2. **A web mode.** A small HTTP server in front of the same Rust code, and
   a version of `src/lib/api.ts` that uses `fetch` when it is not inside
   Tauri. What helps: all 27 commands are called from that one file, and
   speech already reaches the UI as audio data that the page plays. What
   needs work: the commands take Tauri's state and must become plain
   functions; Settings uses a native folder picker and "show in folder";
   backups need download and upload instead of paths; and there is no
   login, and one shared profile, so it wants a password or a reverse
   proxy in front. The image would be small: one binary, `content.db`,
   the built UI and a Piper voice, with the progress folder as a volume.

The second is the one worth building if the point is to use Tilde from a
phone or another computer. Neither has been started.
