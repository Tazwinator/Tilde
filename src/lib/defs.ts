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
