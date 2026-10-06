// @ts-nocheck -- node:test types aren't installed; this runs under `node --test`
import { test } from "node:test";
import assert from "node:assert/strict";
import { decodeText } from "../src/lib/decode.ts";

const SPANISH = "¿Mañana? Sí, «ahí» está el niño… €";

test("reads UTF-8, with or without a BOM", () => {
  const utf8 = new TextEncoder().encode(SPANISH);
  assert.equal(decodeText(utf8), SPANISH);
  assert.equal(decodeText(new Uint8Array([0xef, 0xbb, 0xbf, ...utf8])), SPANISH);
});

test("reads Windows-1252 / Latin-1 subtitles instead of mangling them", () => {
  // the same text as a Windows tool would save it: one byte per character
  const cp1252 = new Uint8Array([
    0xbf, 0x4d, 0x61, 0xf1, 0x61, 0x6e, 0x61, 0x3f, 0x20, 0x53, 0xed, 0x2c, 0x20, 0xab, 0x61, 0x68, 0xed, 0xbb,
    0x20, 0x65, 0x73, 0x74, 0xe1, 0x20, 0x65, 0x6c, 0x20, 0x6e, 0x69, 0xf1, 0x6f, 0x85, 0x20, 0x80,
  ]);
  assert.equal(decodeText(cp1252), SPANISH);
});

test("reads UTF-16 with a byte-order mark", () => {
  const le = [0xff, 0xfe];
  for (const ch of SPANISH) le.push(ch.charCodeAt(0) & 0xff, ch.charCodeAt(0) >> 8);
  assert.equal(decodeText(new Uint8Array(le)), SPANISH);
});
