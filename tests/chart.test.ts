// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { columnXs, dayLabel, linePath, parseDay, scaleYs, wordsChart } from "../src/lib/chart.ts";

const point = (label, value = 1) => ({ label, value });

/** `n` consecutive days from `from` (YYYY-MM-DD), one point each. */
function daily(from, n) {
  const start = parseDay(from);
  return Array.from({ length: n }, (_, i) => point(new Date((start + i) * 86_400_000).toISOString().slice(0, 10), i));
}

test("dates are read and written without a timezone getting in the way", () => {
  assert.equal(parseDay("1970-01-02"), 1);
  assert.equal(parseDay("2026-03-01") - parseDay("2026-02-28"), 1);
  assert.equal(dayLabel("2026-10-05", false), "5 oct");
  assert.equal(dayLabel("2026-09-15", true), "15 sept 26");
  assert.equal(dayLabel("2005-01-02", true), "2 ene 05");
});

test("no points: nothing to draw", () => {
  assert.deepEqual(wordsChart([]), { xs: [], ticks: [], withYear: false });
});

test("one point sits in the middle with its date", () => {
  const chart = wordsChart([point("2026-10-05", 12)]);
  assert.deepEqual(chart.xs, [160]);
  assert.deepEqual(chart.ticks, [{ x: 160, label: "5 oct", anchor: "middle" }]);
  assert.deepEqual(scaleYs([point("2026-10-05", 12)]).map(Number.isFinite), [true]);
});

test("more than a year of daily points gets a handful of labels, not one each", () => {
  const points = daily("2025-06-01", 400);
  const chart = wordsChart(points);
  assert.equal(chart.xs.length, 400);
  assert.ok(chart.xs.every(Number.isFinite));
  assert.ok(chart.ticks.length >= 2 && chart.ticks.length <= 5, `${chart.ticks.length} labels`);
  assert.deepEqual(chart.ticks[0], { x: 8, label: "1 jun 25", anchor: "start" });
  assert.deepEqual(chart.ticks.at(-1), { x: 312, label: "5 jul 26", anchor: "end" });
  for (let i = 1; i < chart.ticks.length; i++) {
    assert.ok(chart.ticks[i].x - chart.ticks[i - 1].x >= 50, "labels too close");
  }
  assert.equal(chart.withYear, true);
});

test("the year shows only when the points span more than one", () => {
  assert.equal(wordsChart(daily("2026-01-01", 300)).withYear, false);
  const acrossNewYear = wordsChart([point("2025-12-30"), point("2026-01-02")]);
  assert.equal(acrossNewYear.withYear, true);
  assert.equal(acrossNewYear.ticks.at(-1).label, "2 ene 26");
});

test("points are placed by date, so a break in play looks like one", () => {
  const { xs } = wordsChart([point("2026-01-01"), point("2026-01-02"), point("2026-03-01")]);
  assert.equal(xs[0], 8);
  assert.equal(xs[2], 312);
  // one day out of 59, not half-way along
  assert.ok(xs[1] - xs[0] < 10, `second point at ${xs[1]}`);
});

test("a label that would run into its neighbour is left out", () => {
  // 70 days into a year: 58 units right of the first label, which is not
  // room enough for two dates with a year in them
  const chart = wordsChart([point("2025-12-01"), point("2026-02-09"), point("2026-12-01")]);
  assert.deepEqual(chart.ticks.map((t) => t.label), ["1 dic 25", "1 dic 26"]);
  // the same gap within one year is two short labels, which fit
  const short = wordsChart([point("2026-01-01"), point("2026-03-12"), point("2026-12-31")]);
  assert.deepEqual(short.ticks.map((t) => t.label), ["1 ene", "12 mar", "31 dic"]);
  // the newest point always keeps its label; the one in its way goes
  const late = wordsChart([point("2026-01-01"), point("2026-12-01"), point("2026-12-31")]);
  assert.deepEqual(late.ticks.map((t) => t.label), ["1 ene", "31 dic"]);
});

test("weekly charts put each point in the middle of its column", () => {
  assert.deepEqual(columnXs(8), [20, 60, 100, 140, 180, 220, 260, 300]);
  assert.deepEqual(columnXs(0), []);
});

test("heights scale to the largest value and never divide by zero", () => {
  assert.deepEqual(scaleYs([point("a", 0), point("b", 50), point("c", 100)]), [124, 66, 8]);
  // all zero: a flat line along the bottom
  assert.deepEqual(scaleYs([point("a", 0), point("b", 0)]), [124, 124]);
  assert.equal(linePath([8, 312], [124, 8]), "M8.0,124.0 L312.0,8.0");
});
