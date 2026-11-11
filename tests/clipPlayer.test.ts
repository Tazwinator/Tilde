// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { createClipPlayer } from "../src/lib/clipPlayer.ts";

function setup() {
  const playing = new Set();
  const started = [];
  const asked = [];
  const player = createClipPlayer({
    tts(text) {
      let resolve;
      const promise = new Promise((res) => (resolve = res));
      asked.push({ text, resolve });
      return promise;
    },
    start(base64) {
      started.push(base64);
      playing.add(base64);
      return { stop: () => playing.delete(base64) };
    },
  });
  return { player, playing, started, asked };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));

test("a new clip stops the one before, so replaying can't overlap", () => {
  const t = setup();
  t.player.play("uno");
  t.player.play("dos");
  assert.deepEqual([...t.playing], ["dos"]);
  t.player.play("dos");
  assert.equal(t.playing.size, 1);
  assert.equal(t.started.length, 3);
});

test("without a wav the text is spoken", async () => {
  const t = setup();
  t.player.play(null, "hola");
  assert.deepEqual(
    t.asked.map((a) => a.text),
    ["hola"],
  );
  assert.equal(t.playing.size, 0);
  t.asked[0].resolve("wav-hola");
  await settle();
  assert.deepEqual([...t.playing], ["wav-hola"]);
  // and it can be stopped like any other clip
  t.player.stop();
  assert.equal(t.playing.size, 0);
});

test("speech that arrives after the page has stopped the audio stays silent", async () => {
  const t = setup();
  t.player.play(null, "hola");
  t.player.stop();
  t.asked[0].resolve("wav-hola");
  await settle();
  assert.deepEqual(t.started, []);
});

test("speech that arrives after a newer clip doesn't talk over it", async () => {
  const t = setup();
  t.player.play(null, "primera");
  t.player.play("segunda");
  t.asked[0].resolve("wav-primera");
  await settle();
  assert.deepEqual([...t.playing], ["segunda"]);

  // two requests for speech: only the newer one plays
  t.player.play(null, "tercera");
  t.player.play(null, "cuarta");
  t.asked[1].resolve("wav-tercera");
  t.asked[2].resolve("wav-cuarta");
  await settle();
  assert.deepEqual([...t.playing], ["wav-cuarta"]);
});

test("no wav, no text, no engine: nothing plays and nothing breaks", async () => {
  const t = setup();
  t.player.play(null);
  assert.equal(t.asked.length, 0);
  t.player.play(null, "hola");
  t.asked[0].resolve(null);
  await settle();
  assert.deepEqual(t.started, []);
  t.player.stop();
  t.player.stop();
});
