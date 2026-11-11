// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";

// A browser that still holds the flag Tilde used to keep there. It has to be
// in place before sfx.ts loads, hence the dynamic import below.
const stored = new Map([["tilde.sound.enabled", "1"]]);
const writes = [];
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (k) => stored.get(k) ?? null,
    setItem: (k, v) => {
      writes.push(k);
      stored.set(k, String(v));
    },
    removeItem: (k) => stored.delete(k),
  },
});
const { forgetLegacySoundFlag, setSfxEnabled, sfx, sfxEnabled } = await import("../src/lib/sfx.ts");

test("sound is off until the saved setting says otherwise, whatever the browser remembers", () => {
  assert.equal(sfxEnabled(), false);
  // muted means silent: nothing reaches for an audio device (there is no `window` here to give one)
  sfx.click();
  sfx.fanfare();

  setSfxEnabled(true);
  assert.equal(sfxEnabled(), true);
  setSfxEnabled(false);
  assert.equal(sfxEnabled(), false);
  assert.deepEqual(writes, [], "the setting lives in the database, not in localStorage");
});

test("the flag an older Tilde left in the browser is cleared away", () => {
  forgetLegacySoundFlag();
  assert.equal(stored.has("tilde.sound.enabled"), false);
});
