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

/** The answer that puts `detail` on screen (null takes the card down). */
const shown = (detail) => ({ current: true, detail });
/** The answer of a request the player has since moved on from. */
const STALE = { current: false };

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
    assert.deepEqual(await changing, shown(detailOf(1, true)));
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
    assert.deepEqual(await opening, shown(detailOf(2)));
    api.pending("wordDetail", 1).resolve(detailOf(1, true));
    assert.deepEqual(await changing, STALE, `${action}: word 1's card came back over word 2's`);
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
    assert.deepEqual(await changing, STALE);
    api.pending("wordDetail", 2).resolve(detailOf(2));
    assert.deepEqual(await opening, shown(detailOf(2)));
  }
});

test("of two words opened, the later one is shown even if it answers first", async () => {
  const api = fakeApi();
  const card = createWordCard(api);
  const first = card.open(1);
  const second = card.open(2);
  api.pending("wordDetail", 2).resolve(detailOf(2));
  assert.deepEqual(await second, shown(detailOf(2)));
  api.pending("wordDetail", 1).resolve(detailOf(1));
  assert.deepEqual(await first, STALE);
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
  assert.deepEqual(await resetting, shown(detailOf(1, false)));
  afterMark.resolve(detailOf(1, true));
  assert.deepEqual(await marking, STALE, "the card from before the reset replaced the one after it");
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

// `word_detail` answers null for a word id the dictionary doesn't have (a
// word that left the list in an update); marking or resetting such a word
// resolves and does nothing.
test("a word the dictionary no longer has takes its card down", async () => {
  const api = fakeApi();
  const card = createWordCard(api);
  const opening = card.open(1);
  api.pending("wordDetail", 1).resolve(null);
  // not the answer of a request that was overtaken: that one leaves the screen alone
  assert.deepEqual(await opening, shown(null));
  assert.notDeepEqual(await opening, STALE);

  for (const [action, write] of CHANGES) {
    const api = fakeApi();
    const card = createWordCard(api);
    const changing = card[action](1);
    api.pending(write, 1).resolve();
    await settle();
    api.pending("wordDetail", 1).resolve(null);
    assert.deepEqual(await changing, shown(null), `${action}: the card of a word that is gone stayed up`);
  }
});

test("a missing word asked for earlier doesn't take down the card opened since", async () => {
  const api = fakeApi();
  const card = createWordCard(api);
  const gone = card.open(1);
  const opening = card.open(2);
  api.pending("wordDetail", 2).resolve(detailOf(2));
  assert.deepEqual(await opening, shown(detailOf(2)));
  api.pending("wordDetail", 1).resolve(null);
  assert.deepEqual(await gone, STALE);
});
