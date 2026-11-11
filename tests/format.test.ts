// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { accuracyRing, dueIn, plural, sessionMinutes } from "../src/lib/format.ts";

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

test("a session's minutes are rounded down, as Home adds them up", () => {
  // 121 s of answering: Home's total goes up by 2
  assert.deepEqual(sessionMinutes(121 / 60), { count: "2", unit: "minutos" });
  // rounded to the nearest this said 3, and Home then added 2
  assert.deepEqual(sessionMinutes(2.6), { count: "2", unit: "minutos" });
  assert.deepEqual(sessionMinutes(1), { count: "1", unit: "minuto" });
  assert.deepEqual(sessionMinutes(1.99), { count: "1", unit: "minuto" });
  assert.deepEqual(sessionMinutes(12), { count: "12", unit: "minutos" });
});

test("less than a minute is not no time at all", () => {
  assert.deepEqual(sessionMinutes(0.98), { count: "<1", unit: "minuto" });
  assert.deepEqual(sessionMinutes(0), { count: "<1", unit: "minuto" });
});

test("a session with no answers has no accuracy to show, which is not 0%", () => {
  // only intro cards were seen: the backend sends rounds 0, accuracy 0
  assert.deepEqual(accuracyRing(0, 0), { label: "—", fill: null });
  // every answer wrong is a real 0%
  assert.deepEqual(accuracyRing(4, 0), { label: "0%", fill: 0 });
  assert.deepEqual(accuracyRing(3, 2 / 3), { label: "67%", fill: 2 / 3 });
  assert.deepEqual(accuracyRing(5, 1), { label: "100%", fill: 1 });
});
