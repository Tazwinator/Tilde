// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { pillShape, textSize } from "../src/lib/defs.ts";

test("long pill texts get a smaller font and a rounded box", () => {
  assert.equal(pillShape("¡Perfecto!"), "text-2xl rounded-full");
  assert.equal(pillShape("x".repeat(40)), "text-lg rounded-3xl");
  assert.equal(pillShape("x".repeat(120)), "text-base rounded-3xl");
});

test("the pill changes shape at the same lengths as the answer buttons change size", () => {
  for (const n of [28, 29, 60, 61]) {
    const text = "x".repeat(n);
    assert.ok(pillShape(text).startsWith(textSize(text) + " "), `${n} characters`);
  }
  assert.equal(pillShape("x".repeat(28)), "text-2xl rounded-full");
  assert.equal(pillShape("x".repeat(29)), "text-lg rounded-3xl");
  assert.equal(pillShape("x".repeat(60)), "text-lg rounded-3xl");
  assert.equal(pillShape("x".repeat(61)), "text-base rounded-3xl");
});
