// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { activates } from "../src/lib/keys.ts";

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
