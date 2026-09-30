// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
// Frontend unit tests: `npm test` (Node's built-in runner).
import { test } from "node:test";
import assert from "node:assert/strict";
import { grade, norm } from "../src/lib/grading.ts";

test("ñ is its own letter", () => {
  assert.equal(grade("ano", "año"), "wrong");
  assert.equal(grade("nino", "niño"), "wrong");
  assert.equal(grade("año", "año"), "right");
});

test("missing accents are reported separately from wrong answers", () => {
  assert.equal(grade("hablo", "habló"), "accents");
  assert.equal(grade("este", "esté"), "accents");
  assert.equal(grade("pinguino", "pingüino"), "accents");
  assert.equal(grade("habló", "habló"), "right");
  assert.equal(grade("hablé", "habló"), "wrong");
});

test("case, spacing and punctuation don't matter", () => {
  assert.equal(grade("  ¿Dónde   ESTÁ el niño? ", "¿Dónde está el niño?"), "right");
  assert.equal(grade("dijo hola", "Dijo: «Hola»…"), "right");
  assert.equal(norm("¡Vale, “vale”!"), "vale vale");
});

test("decomposed input matches precomposed answers", () => {
  assert.equal(grade("año", "año"), "right");
});
