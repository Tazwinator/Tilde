/**
 * Which word's card Explorar shows. The player asks for a card by opening a
 * word, and again by marking or resetting the open one, whose card is read
 * afresh once the change is saved. The answers can come back in any order,
 * so each request takes a number as it is made, and its answer counts only
 * while no later request has been made.
 *
 * No runtime imports, so `node --test` can load it.
 */
import type { WordDetail } from "./contract";

export interface WordCardApi {
  /** null for a word the dictionary doesn't have */
  wordDetail(wordId: number): Promise<WordDetail | null>;
  markWordKnown(wordId: number): Promise<void>;
  resetWord(wordId: number): Promise<void>;
}

/**
 * What a request for a card came to. `current` is false when the player has
 * asked for another card in the meantime: what is on screen then stays.
 * Otherwise `detail` goes on screen, and a null there takes the card down:
 * the dictionary has no such word, and the card of one that is gone must
 * not stay up as if it could still be marked or reset.
 */
export type CardAnswer = { current: false } | { current: true; detail: WordDetail | null };

export function createWordCard(api: WordCardApi) {
  let asked = 0;

  async function read(wordId: number, mine: number): Promise<CardAnswer> {
    const detail = await api.wordDetail(wordId);
    return mine === asked ? { current: true, detail } : { current: false };
  }

  return {
    open: (wordId: number) => read(wordId, ++asked),

    // The number is taken when the button is pressed, not when the card is
    // read again: a word opened while the change was being saved is the
    // later request, though its card may well arrive first.
    async markKnown(wordId: number) {
      const mine = ++asked;
      await api.markWordKnown(wordId);
      return read(wordId, mine);
    },

    async reset(wordId: number) {
      const mine = ++asked;
      await api.resetWord(wordId);
      return read(wordId, mine);
    },
  };
}
