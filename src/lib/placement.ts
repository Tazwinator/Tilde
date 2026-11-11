/**
 * The placement quiz: what can be chosen, and what is sent for it. No
 * runtime imports, so `node --test` can load it.
 */
import type { PlacementAnswer, PlacementItem, PlacementResult } from "./contract";

/** "No lo sé", where a chosen option's index would be. */
export const DONT_KNOW = -1;

/**
 * The key for "No lo sé". Not the digit after the last option: a question
 * short of options would then turn that option's key into "No lo sé". Zero
 * is never an option's key (see optionFromKey).
 */
export const DONT_KNOW_KEY = "0";

/** "No lo sé" as keys.ts's choiceFromKey takes it: its key, and the choice it makes. */
export const DONT_KNOW_CHOICE = { key: DONT_KNOW_KEY, choice: DONT_KNOW } as const;

/**
 * What is sent for a question. "No lo sé" is a miss for that word: the
 * placement credits right answers up to the second miss, so saying so costs
 * what a wrong guess does, and unlike a lucky guess it can't mark words the
 * player has never seen as known.
 */
export function answerFor(item: Pick<PlacementItem, "wordId" | "answerIndex">, chosen: number): PlacementAnswer {
  return { wordId: item.wordId, correct: chosen !== DONT_KNOW && chosen === item.answerIndex };
}

/**
 * "Empiezo de cero": no question is asked (the quiz is never started), so
 * there is no answer to send. The placement is saved as done with nothing
 * marked as known, and the first session starts with the commonest words.
 */
export function startFromScratch(api: { placementSubmit(answers: PlacementAnswer[]): Promise<PlacementResult> }): Promise<PlacementResult> {
  return api.placementSubmit([]);
}
