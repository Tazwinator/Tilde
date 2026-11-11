// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/app.css", import.meta.url), "utf8");

// A source check: there is no DOM here to focus anything in. Unlayered CSS
// beats every Tailwind utility, so a focus rule outside a layer overrides
// each element's own `outline-none` and `rounded-*`.
test("the focus ring sits in the base layer and leaves each element's shape alone", () => {
  assert.equal(css.split(":focus-visible {").length - 1, 1, "one :focus-visible rule");
  const layer = css.indexOf("@layer base {");
  assert.notEqual(layer, -1, "an @layer base block");
  // the block closes at the first "}" at the start of a line
  const base = css.slice(layer, css.indexOf("\n}", layer));
  const rule = base.indexOf(":focus-visible {");
  assert.notEqual(rule, -1, ":focus-visible is inside @layer base");
  const body = base.slice(rule, base.indexOf("}", rule));
  assert.match(body, /outline:/);
  assert.ok(!body.includes("border-radius"), "the ring must not reshape what it surrounds");
});
