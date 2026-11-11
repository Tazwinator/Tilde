//! Stable word ids.
//!
//! The app keys a player's progress by `words.id`, so an id has to mean the
//! same word in every build, whatever happens to the word list or its order.
//! `curation/word_ids.tsv` records the id of every lemma ever selected; a
//! build looks its lemmas up there and only new lemmas get new ids.

use std::collections::{HashMap, HashSet};
use std::path::Path;

const HEADER: &str = "\
# Stable word ids. The app's progress (cards, reviews, mined words) is keyed by
# these ids, so an id must never change or be reused once it has shipped.
# Written by the pipeline: one line per lemma ever selected, lemma<TAB>id,
# sorted by id. Lines are never removed; a lemma that leaves the list keeps
# its line, and gets its id back if it returns.
# Renaming a headword (same word): before the build that added the new lemma
# ships, put the new lemma on the old line, delete the line the build added,
# and rebuild.
";

#[derive(Debug, PartialEq)]
pub struct Registry {
    by_lemma: HashMap<String, i64>,
    next: i64,
}

impl Registry {
    /// `lemma<TAB>id` lines; `#` comments and blank lines are skipped. Anything
    /// else that doesn't parse is an error: guessing here would renumber words.
    pub fn parse(text: &str) -> Result<Registry, String> {
        let mut by_lemma = HashMap::new();
        let mut ids = HashSet::new();
        for (n, line) in text.lines().enumerate() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let at = |what: &str| format!("line {}: {what}: {line:?}", n + 1);
            let (lemma, id) = line.split_once('\t').ok_or_else(|| at("no tab"))?;
            let id: i64 = id.parse().map_err(|_| at("id is not a number"))?;
            if id < 1 || lemma.is_empty() {
                return Err(at("empty lemma or id below 1"));
            }
            if !ids.insert(id) {
                return Err(at("id used twice"));
            }
            if by_lemma.insert(lemma.to_string(), id).is_some() {
                return Err(at("lemma listed twice"));
            }
        }
        let next = ids.iter().max().map_or(1, |max| max + 1);
        Ok(Registry { by_lemma, next })
    }

    /// A missing registry is an error, not an empty one: starting over would
    /// give every word a new id.
    pub fn load(path: &Path) -> Result<Registry, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        Registry::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The id of each of `lemmas`, in order: its registered id, or the next
    /// unused one (then registered too).
    pub fn assign<'a>(&mut self, lemmas: impl IntoIterator<Item = &'a str>) -> Vec<i64> {
        lemmas
            .into_iter()
            .map(|lemma| {
                if let Some(id) = self.by_lemma.get(lemma) {
                    return *id;
                }
                let id = self.next;
                self.next += 1;
                self.by_lemma.insert(lemma.to_string(), id);
                id
            })
            .collect()
    }

    pub fn to_tsv(&self) -> String {
        let mut entries: Vec<(&String, &i64)> = self.by_lemma.iter().collect();
        entries.sort_by_key(|(_, id)| **id);
        let mut out = String::from(HEADER);
        for (lemma, id) in entries {
            out.push_str(&format!("{lemma}\t{id}\n"));
        }
        out
    }

    /// Written under a temporary name and renamed, like the database.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("tsv.tmp");
        std::fs::write(&tmp, self.to_tsv()).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("rename to {}: {e}", path.display()))
    }
}

/// A word as listed in a content database.
#[derive(Debug, Clone)]
pub struct Listed {
    pub lemma: String,
    pub id: i64,
    pub rank: i64,
    pub gloss: String,
}

/// Glosses are compared for the words a person reads through after a rules
/// change: the commonest.
const GLOSS_REVIEW_RANK: i64 = 5000;

/// What a rebuild changes against the database it replaces: which lemmas
/// entered and left, whether any id moved (it never should), pairs that
/// look like one word under a new headword, which a person has to decide,
/// and the commonest words whose gloss changed.
pub fn report(before: &[Listed], after: &[Listed]) -> String {
    if before.is_empty() {
        return String::new();
    }
    let old: HashMap<&str, &Listed> = before.iter().map(|w| (w.lemma.as_str(), w)).collect();
    let new: HashMap<&str, &Listed> = after.iter().map(|w| (w.lemma.as_str(), w)).collect();
    let entered: Vec<&Listed> = after
        .iter()
        .filter(|w| !old.contains_key(w.lemma.as_str()))
        .collect();
    let left: Vec<&Listed> = before
        .iter()
        .filter(|w| !new.contains_key(w.lemma.as_str()))
        .collect();
    let moved: Vec<&Listed> = after
        .iter()
        .filter(|w| old.get(w.lemma.as_str()).is_some_and(|o| o.id != w.id))
        .collect();

    let line = |w: &Listed| {
        format!(
            "    {} (id {}, rank {}): {}\n",
            w.lemma, w.id, w.rank, w.gloss
        )
    };
    let mut out = format!(
        "word list against the committed database: {} kept, {} entered, {} left\n",
        after.len() - entered.len(),
        entered.len(),
        left.len()
    );
    if !moved.is_empty() {
        out.push_str(&format!(
            "  ERROR: {} lemmas would change id; do not ship this build\n",
            moved.len()
        ));
        moved.iter().take(20).for_each(|w| out.push_str(&line(w)));
    }
    for (title, list) in [("entered", &entered), ("left", &left)] {
        if !list.is_empty() {
            out.push_str(&format!("  {title}:\n"));
            list.iter().for_each(|w| out.push_str(&line(w)));
        }
    }
    let renames: Vec<(&Listed, &Listed)> = left
        .iter()
        .flat_map(|l| entered.iter().map(move |e| (*l, *e)))
        .filter(|(l, e)| {
            format!("{}se", e.lemma) == l.lemma
                || format!("{}se", l.lemma) == e.lemma
                || (!l.gloss.is_empty() && l.gloss == e.gloss)
        })
        .collect();
    if !renames.is_empty() {
        out.push_str(
            "  possibly the same word under a new headword (see word_ids.tsv on renaming):\n",
        );
        for (l, e) in renames {
            out.push_str(&format!(
                "    {} (id {}) -> {} (id {})\n",
                l.lemma, l.id, e.lemma, e.id
            ));
        }
    }
    let reworded: Vec<(&Listed, &Listed)> = after
        .iter()
        .filter(|w| w.rank <= GLOSS_REVIEW_RANK)
        .filter_map(|w| Some((*old.get(w.lemma.as_str())?, w)))
        .filter(|(o, w)| o.gloss != w.gloss)
        .collect();
    if !reworded.is_empty() {
        out.push_str(&format!(
            "  glosses changed in the top {GLOSS_REVIEW_RANK}: {}\n",
            reworded.len()
        ));
        for (o, w) in reworded {
            out.push_str(&format!(
                "    {} (id {}, rank {}): {} -> {}\n",
                w.lemma, w.id, w.rank, o.gloss, w.gloss
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(lemma: &str, id: i64, rank: i64, gloss: &str) -> Listed {
        Listed {
            lemma: lemma.to_string(),
            id,
            rank,
            gloss: gloss.to_string(),
        }
    }

    #[test]
    fn report_lists_what_entered_left_and_looks_renamed() {
        let before = [
            listed("casa", 1, 1, "house"),
            listed("arrepentirse", 2, 2, "to repent"),
            listed("bob", 3, 3, "bob haircut"),
        ];
        let after = [
            listed("casa", 1, 1, "house"),
            listed("fuera", 5, 2, "outside"),
            listed("arrepentir", 4, 3, "to regret"),
        ];
        let text = report(&before, &after);
        assert!(text.contains("1 kept, 2 entered, 2 left"), "{text}");
        assert!(text.contains("fuera (id 5, rank 2): outside"), "{text}");
        assert!(text.contains("bob (id 3, rank 3)"), "{text}");
        assert!(
            text.contains("arrepentirse (id 2) -> arrepentir (id 4)"),
            "{text}"
        );
        assert!(!text.contains("ERROR"), "{text}");
        // an id that moved is the one thing that must never ship
        let moved = report(&before, &[listed("casa", 9, 1, "house")]);
        assert!(moved.contains("ERROR: 1 lemmas would change id"), "{moved}");
        assert_eq!(report(&[], &after), "");
    }

    #[test]
    fn report_lists_changed_glosses() {
        let before = [
            listed("patata", 2320, 2320, "piece of rubbish"),
            listed("casa", 1, 1, "house"),
            listed("acatar", 11999, 5001, "to obey"),
        ];
        let after = [
            listed("casa", 1, 1, "house"),
            listed("patata", 2320, 2320, "potato; piece of rubbish"),
            // past the range a person reads through
            listed("acatar", 11999, 5001, "to comply with, to obey"),
        ];
        let text = report(&before, &after);
        assert!(
            text.contains("  glosses changed in the top 5000: 1\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "    patata (id 2320, rank 2320): piece of rubbish -> potato; piece of rubbish\n"
            ),
            "{text}"
        );
        assert!(!text.contains("acatar"), "{text}");
        assert!(!text.contains("casa"), "{text}");
    }

    #[test]
    fn registered_lemmas_keep_their_ids_when_ranks_shift() {
        let mut r = Registry::parse("a\t1\nb\t2\nc\t3\n").unwrap();
        assert_eq!(r.assign(["c", "a", "d"]), [3, 1, 4]);
        // "b" left the list but keeps its line, so it gets 2 back if it returns
        assert!(r.to_tsv().contains("\nb\t2\n"));
        assert_eq!(r.assign(["b", "d"]), [2, 4]);
    }

    #[test]
    fn retired_ids_are_never_reused() {
        let mut r = Registry::parse("a\t1\nb\t5\n").unwrap();
        assert_eq!(r.assign(["c", "e"]), [6, 7]);
    }

    #[test]
    fn accents_and_case_make_different_lemmas() {
        let mut r = Registry::parse("papa\t1\n").unwrap();
        assert_eq!(r.assign(["papá", "Papa", "papa"]), [2, 3, 1]);
    }

    #[test]
    fn parse_rejects_bad_registries() {
        for bad in [
            "a 1\n",
            "a\t0\n",
            "a\tx\n",
            "\t3\n",
            "a\t1\na\t2\n",
            "a\t1\nb\t1\n",
        ] {
            assert!(Registry::parse(bad).is_err(), "accepted {bad:?}");
        }
        let r = Registry::parse("# comment\n\na\t4\n").unwrap();
        assert_eq!(r.next, 5);
        assert_eq!(Registry::parse("").unwrap().next, 1);
    }

    #[test]
    fn tsv_round_trips_sorted_by_id() {
        let mut r = Registry::parse("b\t7\na\t2\n").unwrap();
        r.assign(["c"]);
        let tsv = r.to_tsv();
        let lines: Vec<&str> = tsv.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(lines, ["a\t2", "b\t7", "c\t8"]);
        assert_eq!(Registry::parse(&tsv).unwrap(), r);
    }

    #[test]
    fn the_committed_registry_parses() {
        let r = Registry::parse(include_str!("../curation/word_ids.tsv")).unwrap();
        assert!(r.by_lemma.len() >= 12_000);
        assert_eq!(r.by_lemma["de"], 1);
    }
}
