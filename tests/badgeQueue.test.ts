// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
// The queue behind the badge toasts, with a fake clock.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createBadgeQueue, MAX_TOASTS, SOUND_GAP_MS, spaced, TOAST_MS } from "../src/lib/badgeQueue.ts";
import { PLAY_REVEAL_MS, SIDECAR_REVEAL_MS } from "../src/lib/sessionFlow.ts";

function setup() {
  let now = 0;
  let nextId = 0;
  const timers = new Map();
  const showing = [];
  let sounds = 0;
  const queue = createBadgeQueue(showing, {
    setTimeout(fn, ms) {
      timers.set(++nextId, { at: now + ms, fn });
      return nextId;
    },
    clearTimeout: (id) => timers.delete(id),
    onNew: () => (sounds += 1),
  });
  const tick = (ms) => {
    const end = now + ms;
    for (;;) {
      let due = null;
      for (const [id, t] of timers) if (t.at <= end && (due === null || t.at < due.at)) due = { id, ...t };
      if (!due) break;
      timers.delete(due.id);
      now = due.at;
      due.fn();
    }
    now = end;
  };
  return { queue, showing, tick, sounds: () => sounds, timers };
}

const badge = (id) => ({ id, name: id, description: "", earnedAt: 1 });
const ids = (showing) => showing.map((t) => t.badge.id);

test("a new badge shows for four seconds, with one sound", () => {
  const t = setup();
  t.queue.add([badge("combo-10")]);
  assert.deepEqual(ids(t.showing), ["combo-10"]);
  assert.equal(t.sounds(), 1);
  t.tick(TOAST_MS - 1);
  assert.equal(t.showing.length, 1);
  t.tick(1);
  assert.equal(t.showing.length, 0);
  assert.equal(t.timers.size, 0);
});

test("several badges at once make one sound", () => {
  const t = setup();
  t.queue.add([badge("a"), badge("b")]);
  assert.deepEqual(ids(t.showing), ["a", "b"]);
  assert.equal(t.sounds(), 1);
  t.queue.add([]);
  assert.equal(t.sounds(), 1);
});

test("a badge already on screen or waiting isn't announced twice", () => {
  const t = setup();
  t.queue.add([badge("a")]);
  // the same badge again, as when a finish repeats what a round already gave
  t.queue.add([badge("a")]);
  assert.deepEqual(ids(t.showing), ["a"]);
  assert.equal(t.sounds(), 1);
  t.queue.add([badge("a"), badge("b")]);
  assert.deepEqual(ids(t.showing), ["a", "b"]);
  assert.equal(t.sounds(), 2);
  // once it is gone, earning it again (after a reset, say) is news
  t.tick(TOAST_MS);
  t.queue.add([badge("a")]);
  assert.deepEqual(ids(t.showing), ["a"]);
  assert.equal(t.sounds(), 3);
});

test("at most three show; the rest wait their turn and get their full time", () => {
  const t = setup();
  t.queue.add(["a", "b", "c", "d", "e"].map(badge));
  assert.equal(MAX_TOASTS, 3);
  assert.deepEqual(ids(t.showing), ["a", "b", "c"]);
  // one waiting is still known: no second sound, no second entry
  t.queue.add([badge("e")]);
  assert.equal(t.sounds(), 1);
  t.tick(TOAST_MS);
  assert.deepEqual(ids(t.showing), ["d", "e"]);
  t.tick(TOAST_MS - 1);
  assert.deepEqual(ids(t.showing), ["d", "e"]);
  t.tick(1);
  assert.deepEqual(ids(t.showing), []);
});

test("a click takes a toast down early and lets the next one in", () => {
  const t = setup();
  t.queue.add(["a", "b", "c", "d"].map(badge));
  t.tick(1000);
  t.queue.dismiss(t.showing[1].id);
  assert.deepEqual(ids(t.showing), ["a", "c", "d"]);
  // the dismissed toast's timer is gone: at 4 s only a and c leave
  t.tick(3000);
  assert.deepEqual(ids(t.showing), ["d"]);
  t.tick(1000);
  assert.deepEqual(ids(t.showing), []);
  assert.equal(t.timers.size, 0);
  // an id that is no longer there is ignored
  t.queue.dismiss(999);
});

test("each toast has its own id, also for the same badge shown again", () => {
  const t = setup();
  t.queue.add([badge("a")]);
  const first = t.showing[0].id;
  t.tick(TOAST_MS);
  t.queue.add([badge("a")]);
  assert.notEqual(t.showing[0].id, first);
});

// ---------- the sound ----------

test("the badge sound doesn't play again right after itself", () => {
  let now = 0;
  const played = [];
  const sound = spaced(
    () => played.push(now),
    SOUND_GAP_MS,
    () => now,
  );
  // the toast for the last round's badge
  sound();
  // the summary with a badge of its own: straight after a card with nothing
  // to reveal, or a reveal and a finish later
  now = 20;
  sound();
  now = 900 + 300;
  sound();
  assert.deepEqual(played, [0]);
  now = SOUND_GAP_MS - 1;
  sound();
  assert.deepEqual(played, [0]);
  // far enough apart to be heard as two
  now = SOUND_GAP_MS;
  sound();
  assert.deepEqual(played, [0, SOUND_GAP_MS]);
  // the gap is counted from the last one that sounded, not the last one asked for
  now = SOUND_GAP_MS + 1000;
  sound();
  now = 2 * SOUND_GAP_MS;
  sound();
  assert.deepEqual(played, [0, SOUND_GAP_MS, 2 * SOUND_GAP_MS]);
  // no reveal is longer than the gap, or a summary's sound would depend on the round before it
  assert.ok(SOUND_GAP_MS > Math.max(...Object.values(PLAY_REVEAL_MS), SIDECAR_REVEAL_MS));
});

test("a toast and a summary share one gate, as celebrate.svelte.ts wires them", () => {
  let now = 0;
  let sounds = 0;
  const sound = spaced(
    () => (sounds += 1),
    SOUND_GAP_MS,
    () => now,
  );
  const queue = createBadgeQueue([], { setTimeout: () => 0, clearTimeout: () => {}, onNew: sound });
  queue.add([badge("combo-10")]);
  assert.equal(sounds, 1);
  // the summary's own badge, moments later
  now = 800;
  sound();
  assert.equal(sounds, 1);
  // a session whose only badge is the summary's still sounds
  now = 60_000;
  sound();
  assert.equal(sounds, 2);
});
