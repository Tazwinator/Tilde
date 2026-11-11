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

// Another source check. The session summary packs itself tighter by putting
// `data-tight` on its box when it doesn't fit: that only does anything while
// `short:` answers to the attribute as well as to a short window.
test("`short:` packs a short window, and a box that asks for it with data-tight", () => {
  const start = css.indexOf("@custom-variant short {");
  assert.notEqual(start, -1, "the short variant, in its block form");
  const variant = css.slice(start, css.indexOf("\n}", start));
  assert.match(variant, /@media \(height < 720px\) \{\s*@slot;/, "a window under 720 px high");
  assert.match(variant, /&:where\(\[data-tight\], \[data-tight\] \*\) \{\s*@slot;/, "the box marked data-tight and all inside it");
});

test("a box with more below fades out at its lower edge, in WebKit too", () => {
  const start = css.indexOf(".more-below {");
  assert.notEqual(start, -1, "the .more-below rule");
  const rule = css.slice(start, css.indexOf("}", start));
  const mask = /(?:^|\s)mask-image: (linear-gradient\(to bottom, #000 [^;]+, transparent\));/.exec(rule);
  assert.ok(mask, "a mask that is solid at the top and clear at the bottom");
  // older WebKitGTK only knows the prefixed property: both, with the same value
  assert.ok(rule.includes(`-webkit-mask-image: ${mask[1]};`), "the same mask under -webkit-");
});
