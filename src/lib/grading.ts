/**
 * Answer checking. "ñ" is always its own letter ("ano" is not "año"). Accent
 * marks can matter — in conjugation "habló" and "hablo" are different forms —
 * so each round decides what to do with an answer that only misses accents.
 */

export type Verdict = "right" | "accents" | "wrong";

/** Lowercase, punctuation removed (¿ ¡ « » “ ” … too), spaces collapsed. Keeps accents. */
export function norm(s: string): string {
  return s
    .normalize("NFC")
    .toLowerCase()
    .replace(/[\p{P}\p{S}]/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/** norm() without accent marks (á → a, ü → u). The tilde of ñ (U+0303) stays. */
function withoutAccents(s: string): string {
  return norm(s)
    .normalize("NFD")
    .replace(/[̀-̂̄-ͯ]/g, "")
    .normalize("NFC");
}

export function grade(typed: string, answer: string): Verdict {
  if (norm(typed) === norm(answer)) return "right";
  if (withoutAccents(typed) === withoutAccents(answer)) return "accents";
  return "wrong";
}
