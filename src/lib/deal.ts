// How a round lays out what it is given. No imports, so `node --test` can
// load this file as it is.

export function shuffle<T>(a: readonly T[]): T[] {
  const arr = [...a];
  for (let i = arr.length - 1; i > 0; i--) {
    const j = (Math.random() * (i + 1)) | 0;
    [arr[i], arr[j]] = [arr[j], arr[i]];
  }
  return arr;
}

/**
 * The right-hand column of a match round, never in the same order as the
 * left. Two shuffles land on the same order now and then (1 time in 24 with
 * four pairs), and then every word sits opposite its partner: the round is
 * solved by reading across. Only that one deal is changed, by moving each
 * tile up one place; any other order is left as the shuffle made it.
 */
export function dealUnaligned<L extends { wordId: number }, R extends { wordId: number }>(left: readonly L[], right: readonly R[]): R[] {
  const aligned = right.length === left.length && right.every((t, i) => t.wordId === left[i].wordId);
  if (!aligned || right.length < 2) return [...right];
  return [...right.slice(1), right[0]];
}
