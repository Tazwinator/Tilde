// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { answerFor, DONT_KNOW, DONT_KNOW_CHOICE, DONT_KNOW_KEY, startFromScratch } from "../src/lib/placement.ts";
import { choiceFromKey } from "../src/lib/keys.ts";

const item = (answerIndex, wordId = 40) => ({ wordId, lemma: "del", options: ["a", "b", "c", "d"], answerIndex, band: "A1" });

// a key press as the window listener sees it
const press = (key, over = {}) => ({ key, repeat: false, ctrlKey: false, altKey: false, metaKey: false, ...over });
// what the placement page makes of a key press on a question with `count` options
const choice = (e, count) => choiceFromKey(e, count, DONT_KNOW_CHOICE);

test("«No lo sé» is sent as a wrong answer for that word", () => {
  assert.deepEqual(answerFor(item(2), DONT_KNOW), { wordId: 40, correct: false });
  // wherever the right option sits
  for (const answerIndex of [0, 1, 2, 3]) {
    assert.equal(answerFor(item(answerIndex), DONT_KNOW).correct, false);
  }
});

test("a chosen option is right only if it is the answer", () => {
  assert.deepEqual(answerFor(item(2), 2), { wordId: 40, correct: true });
  assert.deepEqual(answerFor(item(2), 0), { wordId: 40, correct: false });
  assert.deepEqual(answerFor(item(0, 7), 0), { wordId: 7, correct: true });
});

test("0 is «No lo sé» on every question, and 1 to 4 keep their meaning", () => {
  assert.equal(DONT_KNOW_KEY, "0");
  assert.deepEqual(DONT_KNOW_CHOICE, { key: "0", choice: DONT_KNOW });
  for (const count of [2, 3, 4]) {
    assert.equal(choice(press("0"), count), DONT_KNOW, `0 with ${count} options`);
    for (let n = 1; n <= 4; n++) {
      assert.equal(choice(press(String(n)), count), n <= count ? n - 1 : null, `${n} with ${count} options`);
    }
  }
  // the key after the last option of a short question is nothing: not the
  // option that isn't there, and not «No lo sé»
  assert.equal(choice(press("4"), 3), null);
  assert.equal(choice(press("5"), 4), null);
  assert.equal(choice(press("Enter"), 4), null);
  assert.equal(choice(press(" "), 4), null);
});

test("a key held down or a shortcut answers no placement question", () => {
  // held from the question before, «No lo sé» would run through every question left
  assert.equal(choice(press("0", { repeat: true }), 4), null);
  assert.equal(choice(press("1", { repeat: true }), 4), null);
  // Ctrl+0 (the browser's "reset zoom") is not 0
  for (const mod of ["ctrlKey", "altKey", "metaKey"]) {
    assert.equal(choice(press("0", { [mod]: true }), 4), null, `${mod}+0`);
    assert.equal(choice(press("2", { [mod]: true }), 4), null, `${mod}+2`);
  }
  // Shift is no shortcut: on some layouts the digits are typed with it
  assert.equal(choice(press("0", { shiftKey: true }), 4), DONT_KNOW);
  assert.equal(choice(press("1", { shiftKey: true }), 4), 0);
});

test("«Empiezo de cero» sends no answers and never starts the quiz", async () => {
  const calls = [];
  const result = { frontierRank: 1, wordsMarkedKnown: 0, cefrEstimate: "A1" };
  const api = {
    placementStart: async () => {
      calls.push(["placementStart"]);
      return [item(0)];
    },
    placementSubmit: async (answers) => {
      calls.push(["placementSubmit", answers]);
      return result;
    },
  };
  assert.equal(await startFromScratch(api), result);
  assert.deepEqual(calls, [["placementSubmit", []]]);
});

test("«Empiezo de cero» that can't be saved fails, so the page can say so", async () => {
  const api = { placementSubmit: async () => Promise.reject(new Error("disk full")) };
  await assert.rejects(startFromScratch(api), /disk full/);
});
