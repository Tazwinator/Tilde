import type { WordCard, WordHit } from "./contract";

interface Glossy {
  glossEn: string | null;
  glossEs: string | null;
}

/** Pick gloss text per definition_lang setting ("es" → glossEs with EN fallback, else EN first). */
export function glossText(w: Glossy, definitionLang: string): string {
  if (definitionLang === "es") {
    return w.glossEs ?? w.glossEn ?? "";
  }
  return w.glossEn ?? w.glossEs ?? "";
}

export type Glossable = WordCard | WordHit;

const POS_LABELS: Record<string, string> = {
  noun: "sustantivo",
  verb: "verbo",
  adj: "adjetivo",
  adv: "adverbio",
  pron: "pronombre",
  det: "determinante",
  article: "artículo",
  prep: "preposición",
  conj: "conjunción",
  contraction: "contracción",
  num: "numeral",
  particle: "partícula",
  intj: "interjección",
};

/** Spanish label for a part of speech from the content DB ("noun" → "sustantivo"). */
export function posLabel(pos: string): string {
  return POS_LABELS[pos] ?? pos;
}
