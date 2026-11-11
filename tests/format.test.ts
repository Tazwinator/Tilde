// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { dueIn, plural } from "../src/lib/format.ts";

test("a count takes the singular only for one", () => {
  assert.equal(plural(0, "sesión", "sesiones"), "0 sesiones");
  assert.equal(plural(1, "sesión", "sesiones"), "1 sesión");
  assert.equal(plural(2, "sesión", "sesiones"), "2 sesiones");
});

test("a due time is said in the unit that fits it, not as a raw number of days", () => {
  // a word just marked as known: due in 180 days, less the moment it took to ask
  assert.equal(dueIn(179.99999969184674), "en 6 meses");
  assert.equal(dueIn(0.20927301605266554), "en 5 h");
  assert.equal(dueIn(0), "ya");
  assert.equal(dueIn(0.02), "ya");
  assert.equal(dueIn(1 / 24), "en 1 h");
  assert.equal(dueIn(1), "en 1 día");
  assert.equal(dueIn(2.4), "en 2 días");
  assert.equal(dueIn(45), "en 2 meses");
  assert.equal(dueIn(30), "en 1 mes");
  assert.equal(dueIn(400), "en 1 año");
  assert.equal(dueIn(800), "en 2 años");
});
