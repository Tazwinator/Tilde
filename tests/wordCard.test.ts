// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
// Which word's card Explorar shows, with an api whose calls the test answers
// by hand, in whatever order it likes.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createWordCard } from "../src/lib/wordCard.ts";

/** Lets every promise callback that is ready run. */
const settle = () => new Promise((resolve) => setImmediate(resolve));

/** An api whose calls stay open until the test answers them. */
function fakeApi() {
  const calls = [];
  const call = (name) => (wordId) =>
    new Promise((resolve, reject) => {
      calls.push({ name, wordId, resolve, reject });
    });
  return {
    wordDetail: call("wordDetail"),
    markWordKnown: call("markWordKnown"),
    resetWord: call("resetWord"),
    /** every call so far, as "name wordId" */
    asked: () => calls.map((c) => `${c.name} ${c.wordId}`),
    /** The open call of that name for that word. */
    pending(name, wordId) {
      const i = calls.findIndex((c) => !c.done && c.name === name && c.wordId === wordId);
      assert.notEqual(i, -1, `no open call ${name} ${wordId}`);
      calls[i].done = true;
      return calls[i];
    },
  };
}

const detailOf = (wordId, known = false) => ({ word: { wordId, known }, card: null });

const CHANGES = [
  ["markKnown", "markWordKnown"],
  ["reset", "resetWord"],
];

test("a card is read again only once the change to its word is saved", async () => {
  for (const [action, write] of CHANGES) {
    const api = fakeApi();
    const card = createWordCard(api);
    const changing = card[action](1);
    await settle();
    assert.deepEqual(api.asked(), [`${write} 1`], "the card was read before the change was in");
    api.pending(write, 1).resolve();
    await settle();
    assert.deepEqual(api.asked(), [`${write} 1`, "wordDetail 1"]);
    api.pending("wordDetail", 1).resolve(detailOf(1, true));
    assert.deepEqual(await changing, detailOf(1, true));
  }
});

test("a word opened while another is being changed keeps its card", async () => {
  for (const [action, write] of CHANGES) {
    // word 1's card is open; the player presses its button and, before that
    // is saved, clicks word 2 in the list
    const api = fakeApi();
    const card = createWordCard(api);
    const changing = card[action](1);
    const opening = card.open(2);
    api.pending(write, 1).resolve();
    await settle();
    // the reads answered in the order they were made: word 2's card, then
    // word 1's as it is after the change
    api.pending("wordDetail", 2).resolve(detailOf(2));
    assert.deepEqual(await opening, detailOf(2));
    api.pending("wordDetail", 1).resolve(detailOf(1, true));
    assert.equal(await changing, null, `${action}: word 1's card came back over word 2's`);
  }
});

test("the same when the changed word's card is the first to arrive", async () => {
  for (const [action, write] of CHANGES) {
    const api = fakeApi();
    const card = createWordCard(api);
    const changing = card[action](1);
    const opening = card.open(2);
    api.pending(write, 1).resolve();
    await settle();
    api.pending("wordDetail", 1).resolve(detailOf(1, true));
    assert.equal(await changing, null);
    api.pending("wordDetail", 2).resolve(detailOf(2));
    assert.deepEqual(await opening, detailOf(2));
  }
});

test("of two words opened, the later one is shown even if it answers first", async () => {
  const api = fakeApi();
  const card = createWordCard(api);
  const first = card.open(1);
  const second = card.open(2);
  api.pending("wordDetail", 2).resolve(detailOf(2));
  assert.deepEqual(await second, detailOf(2));
  api.pending("wordDetail", 1).resolve(detailOf(1));
  assert.equal(await first, null);
});

test("a word marked and then reset shows as the reset left it", async () => {
  const api = fakeApi();
  const card = createWordCard(api);
  const marking = card.markKnown(1);
  const resetting = card.reset(1);
  api.pending("markWordKnown", 1).resolve();
  api.pending("resetWord", 1).resolve();
  await settle();
  // two reads of the same card are out, one from before the reset; they answer the wrong way round
  const [afterMark, afterReset] = [api.pending("wordDetail", 1), api.pending("wordDetail", 1)];
  afterReset.resolve(detailOf(1, false));
  assert.deepEqual(await resetting, detailOf(1, false));
  afterMark.resolve(detailOf(1, true));
  assert.equal(await marking, null, "the card from before the reset replaced the one after it");
});

test("a change that can't be saved fails out loud, and the card is not read again", async () => {
  for (const [action, write] of CHANGES) {
    const api = fakeApi();
    const card = createWordCard(api);
    const changing = card[action](1);
    api.pending(write, 1).reject(new Error("disk on fire"));
    await assert.rejects(changing, /disk on fire/);
    assert.deepEqual(api.asked(), [`${write} 1`]);
  }
});
