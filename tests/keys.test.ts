// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { activates, choiceFromKey, optionFromKey } from "../src/lib/keys.ts";

// a key press as the window listener sees it, with no DOM behind it
const press = (over = {}) => ({
  key: "Enter",
  code: "Enter",
  repeat: false,
  ctrlKey: false,
  altKey: false,
  metaKey: false,
  target: { tagName: "BODY" },
  ...over,
});

test("Enter or Space on the page activates the card, but not a repeat, a modifier or a focused button", () => {
  assert.equal(activates({ key: "Enter", repeat: false, target: { tagName: "BODY" } }), true);
  assert.equal(activates(press({ key: " ", code: "Space", target: { tagName: "DIV" } })), true);
  // a key still held down from the round before must not skip a card nobody has read
  assert.equal(activates(press({ repeat: true })), false);
  assert.equal(activates(press({ ctrlKey: true })), false);
  assert.equal(activates(press({ altKey: true })), false);
  assert.equal(activates(press({ metaKey: true })), false);
  // on a button or link the press is that control's own click
  assert.equal(activates(press({ target: { tagName: "BUTTON" } })), false);
  assert.equal(activates(press({ target: { tagName: "A" } })), false);
  // in a field it is typing
  assert.equal(activates(press({ target: { tagName: "INPUT" } })), false);
  assert.equal(activates(press({ key: " ", code: "Space", target: { tagName: "TEXTAREA" } })), false);
  assert.equal(activates(press({ target: { tagName: "SELECT" } })), false);
  assert.equal(activates(press({ target: { tagName: "DIV", isContentEditable: true } })), false);
});

test("other keys never activate", () => {
  assert.equal(activates(press({ key: "a", code: "KeyA" })), false);
  assert.equal(activates(press({ key: "1", code: "Digit1" })), false);
  assert.equal(activates(press({ key: "Escape", code: "Escape" })), false);
  assert.equal(activates(press({ key: "Tab", code: "Tab" })), false);
});

test("Shift doesn't matter, and neither does a press with no target", () => {
  assert.equal(activates(press({ shiftKey: true })), true);
  assert.equal(activates(press({ target: null })), true);
  // XHTML-style lowercase tag names are still the same controls
  assert.equal(activates(press({ target: { tagName: "button" } })), false);
});

test("a digit picks an option only if the round has that many", () => {
  assert.equal(optionFromKey("1", 4), 0);
  assert.equal(optionFromKey("4", 4), 3);
  assert.equal(optionFromKey("3", 3), 2);
  // a round short of options: the key for the missing one does nothing
  assert.equal(optionFromKey("4", 3), null);
  assert.equal(optionFromKey("2", 1), null);
  assert.equal(optionFromKey("1", 0), null);
  assert.equal(optionFromKey("0", 4), null);
  assert.equal(optionFromKey("5", 4), null);
  // no round shows more than four, however many it is said to have
  assert.equal(optionFromKey("5", 9), null);
  assert.equal(optionFromKey("a", 4), null);
  assert.equal(optionFromKey("12", 4), null);
  assert.equal(optionFromKey("F1", 4), null);
  assert.equal(optionFromKey(" ", 4), null);
  assert.equal(optionFromKey("", 4), null);
});

test("a question's extra choice has a key of its own, beside the options' digits", () => {
  const other = { key: "x", choice: 99 };
  const key = (key, over = {}) => ({ key, repeat: false, ctrlKey: false, altKey: false, metaKey: false, ...over });
  assert.equal(choiceFromKey(key("x"), 4, other), 99);
  assert.equal(choiceFromKey(key("x"), 0, other), 99);
  // the options are picked as optionFromKey has it
  assert.equal(choiceFromKey(key("1"), 4, other), 0);
  assert.equal(choiceFromKey(key("4"), 3, other), null);
  assert.equal(choiceFromKey(key("y"), 4, other), null);
  // a key still held down, or a shortcut, chooses nothing at all
  assert.equal(choiceFromKey(key("x", { repeat: true }), 4, other), null);
  assert.equal(choiceFromKey(key("3", { repeat: true }), 4, other), null);
  assert.equal(choiceFromKey(key("x", { ctrlKey: true }), 4, other), null);
  assert.equal(choiceFromKey(key("3", { metaKey: true }), 4, other), null);
});
