# content.db — data license

`content.db` is built by `crates/tilde-pipeline` from the open datasets below
and is shared under [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).
The app's code is MIT-licensed separately (see `LICENSE` at the repo root).

| Data | Source | License |
|---|---|---|
| Word frequencies | [FrequencyWords](https://github.com/hermitdave/FrequencyWords) by Hermit Dave, from OpenSubtitles 2018 | CC BY-SA 4.0 |
| Lemmas, glosses, inflections, conjugation tables | [Wiktionary](https://en.wiktionary.org/) contributors, extracted by [kaikki.org](https://kaikki.org/) | CC BY-SA 4.0 (also GFDL) |
| Example sentences | [Tatoeba](https://tatoeba.org/) contributors, via [OPUS](https://opus.nlpl.eu/) (Tatoeba v2023-04-12) | CC BY 2.0 FR |

The glosses were shortened and a few were rewritten by hand
(`crates/tilde-pipeline/curation/`). The exact source files used are recorded
by SHA-256 in the database's `meta` table. The app shows this attribution
under Ajustes → Créditos de los datos.
