// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { dealUnaligned, shuffle } from "../src/lib/deal.ts";

const tiles = (ids, side) => ids.map((wordId) => ({ wordId, text: `${side}${wordId}` }));
const order = (column) => column.map((t) => t.wordId);

/** Every order of `items`. */
function permutations(items) {
  if (items.length <= 1) return [items];
  return items.flatMap((first, i) => permutations([...items.slice(0, i), ...items.slice(i + 1)]).map((rest) => [first, ...rest]));
}

test("the right column of a match round is never in the left column's order", () => {
  for (const n of [2, 3, 4, 5]) {
    const ids = Array.from({ length: n }, (_, i) => 10 + i);
    for (const leftOrder of permutations(ids)) {
      const left = tiles(leftOrder, "es");
      for (const rightOrder of permutations(ids)) {
        const right = tiles(rightOrder, "en");
        const dealt = dealUnaligned(left, right);
        assert.notDeepEqual(order(dealt), leftOrder, `${n} pairs dealt aligned`);
        // the same tiles, each once
        assert.deepEqual([...order(dealt)].sort(), [...ids].sort());
        assert.ok(dealt.every((t) => right.includes(t)));
      }
    }
  }
});

test("a deal that is not aligned is left as the shuffle made it", () => {
  const left = tiles([1, 2, 3, 4], "es");
  // one pair opposite its partner is not a solved round
  const right = tiles([1, 3, 4, 2], "en");
  const dealt = dealUnaligned(left, right);
  assert.deepEqual(order(dealt), [1, 3, 4, 2]);
  assert.notEqual(dealt, right, "a new array: the round's own list is not handed back");
});

test("an aligned deal moves every tile up one place", () => {
  assert.deepEqual(order(dealUnaligned(tiles([7, 8, 9, 6], "es"), tiles([7, 8, 9, 6], "en"))), [8, 9, 6, 7]);
  assert.deepEqual(order(dealUnaligned(tiles([1, 2], "es"), tiles([1, 2], "en"))), [2, 1]);
});

test("one pair, or none, has no other order to be dealt in", () => {
  assert.deepEqual(order(dealUnaligned(tiles([5], "es"), tiles([5], "en"))), [5]);
  assert.deepEqual(dealUnaligned([], []), []);
});

test("the input columns are not changed", () => {
  const left = tiles([1, 2, 3], "es");
  const right = tiles([1, 2, 3], "en");
  dealUnaligned(left, right);
  assert.deepEqual(order(left), [1, 2, 3]);
  assert.deepEqual(order(right), [1, 2, 3]);
});

test("shuffle keeps every item and leaves its input alone", () => {
  const items = [1, 2, 3, 4, 5, 6];
  for (let i = 0; i < 50; i++) {
    const out = shuffle(items);
    assert.deepEqual([...out].sort(), items);
  }
  assert.deepEqual(items, [1, 2, 3, 4, 5, 6]);
});
