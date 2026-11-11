// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
// The session controller behind /play and /sidecar, driven with a fake clock
// and an api whose calls the test answers by hand.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createFlow, initialView, mergeBadges, resolveKind, PLAY_REVEAL_MS } from "../src/lib/sessionFlow.ts";

function fakeClock() {
  let now = 0;
  let nextId = 0;
  const timers = new Map();
  return {
    setTimeout(fn, ms) {
      timers.set(++nextId, { at: now + ms, fn });
      return nextId;
    },
    clearTimeout(id) {
      timers.delete(id);
    },
    now: () => now,
    /** Moves time on, running the timers that come due, in order. */
    tick(ms) {
      const end = now + ms;
      for (;;) {
        let due = null;
        for (const [id, t] of timers) {
          if (t.at <= end && (due === null || t.at < due.at)) due = { id, ...t };
        }
        if (!due) break;
        timers.delete(due.id);
        now = due.at;
        due.fn();
      }
      now = end;
    },
    waiting: () => timers.size,
  };
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Lets every promise callback that is ready run. */
const settle = () => new Promise((resolve) => setImmediate(resolve));

const badge = (id) => ({ id, name: id, description: "", earnedAt: 1 });

const feedback = (over = {}) => ({ correctAnswer: null, xpGained: 10, combo: 1, levelUp: null, newBadges: [], ...over });

const summaryOf = (over = {}) => ({
  xp: 30,
  rounds: 3,
  correct: 3,
  minutes: 1,
  newWords: [],
  accuracy: 1,
  levelUp: null,
  newBadges: [],
  bestCombo: 3,
  ...over,
});

const choice = (id) => ({ type: "choice", id, wordId: id, prompt: "casa", promptLang: "es", options: ["a", "b"], answerIndex: 0, audioBase64: null });

const sessionOf = (n, sessionId = 7) => ({ sessionId, kind: "quick", rounds: Array.from({ length: n }, (_, i) => choice(100 + i)) });

/** A flow over fakes. Every api call is logged and stays open until the test settles it. */
function setup({ requested = "quick", fixedKind, reveal = 700, withSettings = true } = {}) {
  const clock = fakeClock();
  const log = [];
  const open = { settings: [], start: [], submit: [], finish: [] };
  const call = (queue, entry) => {
    log.push(entry);
    const d = deferred();
    queue.push(d);
    return d.promise;
  };
  const api = {
    startSession: (kind) => call(open.start, ["startSession", kind]),
    submitRound: (sid, rid, result) => call(open.submit, ["submitRound", sid, rid, result]),
    finishSession: (sid) => call(open.finish, ["finishSession", sid]),
  };
  if (withSettings) api.getSettings = () => call(open.settings, ["getSettings"]);

  const seen = { celebrated: [], errors: [], feedbacks: [], dones: [] };
  const view = initialView();
  const flow = createFlow(
    view,
    {
      api,
      clock,
      celebrate: (badges) => seen.celebrated.push(badges.map((b) => b.id)),
      reportError: (e) => seen.errors.push(e),
      flashText: (fb, correct, correctText) => (correct ? "¡Bien!" : `era ${correctText}`),
      onFeedback: (fb) => seen.feedbacks.push(fb),
      onDone: (s) => seen.dones.push(s),
      revealMs: () => reveal,
    },
    { requested, fixedKind },
  );
  const calls = (name) => log.filter((c) => c[0] === name);
  return { clock, log, open, view, flow, seen, calls };
}

/** A flow that is showing round 0 of an n-round session. */
async function playing(n = 3, opts = {}) {
  const t = setup(opts);
  void t.flow.start();
  if (t.open.settings[0]) t.open.settings[0].resolve({ definitionLang: "es", sessionLength: "standard" });
  t.open.start[0].resolve(sessionOf(n));
  await settle();
  assert.equal(t.view.phase, "playing");
  return t;
}

/** Answers the round on screen and lets its submit come back. */
async function answerNow(t, correct = true, fb = feedback(), correctText = "a") {
  const at = t.open.submit.length;
  void t.flow.answer(correct, correctText);
  t.open.submit[at].resolve(fb);
  await settle();
}

// ---------- which session ----------

test("a bare /play uses the saved session length", async () => {
  assert.equal(resolveKind(null, "quick"), "quick");
  assert.equal(resolveKind(null, "nonsense"), "standard");
  assert.equal(resolveKind("deep", "quick"), "deep");
  assert.equal(resolveKind("sidecar", "quick"), "quick");
  assert.equal(resolveKind("foo", undefined), "standard");
  assert.equal(resolveKind("review_only", "deep"), "review_only");
  // review_only is something you ask for, not a session length
  assert.equal(resolveKind(null, "review_only"), "standard");

  // and the flow asks for the settings first, then starts that kind
  const t = setup({ requested: null });
  void t.flow.start();
  assert.deepEqual(t.log, [["getSettings"]]);
  t.open.settings[0].resolve({ definitionLang: "es", sessionLength: "deep" });
  await settle();
  assert.deepEqual(t.calls("startSession"), [["startSession", "deep"]]);
  t.open.start[0].resolve(sessionOf(2));
  await settle();
  assert.equal(t.view.phase, "playing");
  assert.equal(t.view.kind, "deep");
  assert.equal(t.view.definitionLang, "es");
});

test("a kind in the URL starts at once, alongside the settings", async () => {
  const t = setup({ requested: "review_only" });
  void t.flow.start();
  assert.deepEqual(t.log, [["getSettings"], ["startSession", "review_only"]]);
  // nothing shows until both are in: rounds need the definition language
  t.open.start[0].resolve(sessionOf(2));
  await settle();
  assert.equal(t.view.phase, "loading");
  t.open.settings[0].resolve({ definitionLang: "es", sessionLength: "quick" });
  await settle();
  assert.equal(t.view.phase, "playing");
  assert.equal(t.view.kind, "review_only");
});

test("a kind the page can't run is ignored in favour of the saved default", async () => {
  const t = setup({ requested: "foo" });
  void t.flow.start();
  t.open.settings[0].resolve({ definitionLang: "en", sessionLength: "quick" });
  await settle();
  assert.deepEqual(t.calls("startSession"), [["startSession", "quick"]]);
});

test("the sidecar's fixed kind needs no settings", async () => {
  const t = setup({ requested: null, fixedKind: "sidecar", withSettings: false });
  void t.flow.start();
  assert.deepEqual(t.log, [["startSession", "sidecar"]]);
  t.open.start[0].resolve(sessionOf(1));
  await settle();
  assert.equal(t.view.phase, "playing");
  assert.equal(t.view.kind, "sidecar");
});

test("start() twice starts one session", async () => {
  const t = setup();
  void t.flow.start();
  void t.flow.start();
  assert.equal(t.calls("startSession").length, 1);
});

test("no rounds means the empty state", async () => {
  const t = setup({ requested: "review_only" });
  void t.flow.start();
  t.open.settings[0].resolve({ definitionLang: "en", sessionLength: "standard" });
  t.open.start[0].resolve(sessionOf(0));
  await settle();
  assert.equal(t.view.phase, "empty");
  assert.equal(t.view.kind, "review_only");
  // an empty session is still closed when the page goes
  t.flow.leave();
  await settle();
  assert.deepEqual(t.calls("finishSession"), [["finishSession", 7]]);
});

test("a session that can't start shows the error state", async () => {
  const t = setup();
  void t.flow.start();
  t.open.settings[0].resolve({ definitionLang: "en", sessionLength: "standard" });
  t.open.start[0].reject(new Error("boom"));
  await settle();
  assert.equal(t.view.phase, "error");
  assert.equal(t.seen.errors.length, 1);
  assert.equal(t.view.session, null);
});

test("when the settings fail, the session that started alongside is closed", async () => {
  const t = setup();
  void t.flow.start();
  t.open.start[0].resolve(sessionOf(2, 42));
  t.open.settings[0].reject(new Error("no settings"));
  await settle();
  assert.equal(t.view.phase, "error");
  assert.equal(t.seen.errors.length, 1);
  assert.deepEqual(t.calls("finishSession"), [["finishSession", 42]]);
  assert.equal(t.view.session, null);
});

// ---------- leaving ----------

test("a session that arrives after leaving is finished and never shown", async () => {
  const t = setup();
  void t.flow.start();
  t.flow.leave();
  t.open.settings[0].resolve({ definitionLang: "es", sessionLength: "standard" });
  t.open.start[0].resolve(sessionOf(3, 99));
  await settle();
  assert.deepEqual(t.calls("finishSession"), [["finishSession", 99]]);
  assert.equal(t.view.session, null);
  assert.equal(t.view.phase, "loading");
  assert.equal(t.view.definitionLang, "en");
  // and a finish that fails there is nobody's business
  t.open.finish[0].reject(new Error("late"));
  await settle();
  assert.equal(t.seen.errors.length, 0);
});

test("leaving before the kind is known starts no session at all", async () => {
  const t = setup({ requested: null });
  void t.flow.start();
  t.flow.leave();
  t.open.settings[0].resolve({ definitionLang: "es", sessionLength: "deep" });
  await settle();
  assert.equal(t.calls("startSession").length, 0);
  assert.equal(t.view.phase, "loading");
});

test("an answer given just before leaving is still graded, then the session finished", async () => {
  const t = await playing(1);
  void t.flow.answer(true, "a");
  assert.equal(t.calls("submitRound").length, 1, "the answer goes out at once, not after the reveal");
  t.flow.leave();
  await settle();
  assert.equal(t.calls("finishSession").length, 0, "the finish waits for the answer in flight");
  t.open.submit[0].resolve(feedback());
  await settle();
  assert.deepEqual(
    t.log.slice(-2).map((c) => c[0]),
    ["submitRound", "finishSession"],
  );
  assert.equal(t.calls("finishSession").length, 1);
  t.open.finish[0].resolve(summaryOf());
  t.clock.tick(5000);
  await settle();
  assert.equal(t.calls("finishSession").length, 1);
  assert.equal(t.seen.dones.length, 0);
  assert.equal(t.seen.feedbacks.length, 0);
  assert.equal(t.view.phase, "playing");
  assert.equal(t.view.summary, null);
  assert.equal(t.view.xp, 0);
});

test("nothing advances after leaving", async () => {
  const t = await playing(3);
  await answerNow(t);
  assert.equal(t.seen.feedbacks.length, 1);
  assert.equal(t.clock.waiting(), 1, "the reveal is running");
  t.flow.leave();
  assert.equal(t.clock.waiting(), 0, "no timer is left running");
  t.clock.tick(5000);
  await settle();
  assert.equal(t.view.idx, 0);
  assert.equal(t.view.flash, null);
  assert.equal(t.seen.feedbacks.length, 1);
  assert.equal(t.seen.dones.length, 0);
  // a late key press on the dying page does nothing
  void t.flow.answer(true, "a");
  t.flow.continue();
  t.flow.setPaused(false);
  await settle();
  assert.equal(t.calls("submitRound").length, 1);
  assert.equal(t.view.idx, 0);
  // the session is closed once
  t.flow.leave();
  await settle();
  assert.equal(t.calls("finishSession").length, 1);
});

test("a round that was waiting behind the exit dialog doesn't appear after leaving", async () => {
  const t = await playing(3);
  t.flow.setPaused(true);
  await answerNow(t);
  t.clock.tick(700);
  t.flow.leave();
  // the dialog closing as the page goes
  t.flow.setPaused(false);
  assert.equal(t.view.idx, 0);
  assert.equal(t.view.flash, null);
  assert.equal(t.clock.waiting(), 0);
});

test("leaving takes down the pill's timer too", async () => {
  const t = await playing(3);
  await answerNow(t);
  t.clock.tick(700);
  assert.ok(t.view.flash);
  assert.equal(t.clock.waiting(), 1);
  t.flow.leave();
  assert.equal(t.clock.waiting(), 0);
});

test("badges from a finish nobody is there to see are still announced", async () => {
  const t = await playing(2);
  await answerNow(t);
  t.flow.leave();
  await settle();
  t.open.finish[0].resolve(summaryOf({ newBadges: [badge("minutes-60")] }));
  await settle();
  assert.deepEqual(t.seen.celebrated, [["minutes-60"]]);
  assert.equal(t.seen.dones.length, 0);
});

test("leaving while the last round's finish is in flight announces its badges and shows nothing", async () => {
  const t = await playing(1);
  await answerNow(t);
  t.clock.tick(700);
  assert.equal(t.calls("finishSession").length, 1);
  t.flow.leave();
  t.open.finish[0].resolve(summaryOf({ newBadges: [badge("words-50")] }));
  await settle();
  assert.equal(t.calls("finishSession").length, 1);
  assert.deepEqual(t.seen.celebrated, [["words-50"]]);
  assert.equal(t.view.phase, "playing");
  assert.equal(t.seen.dones.length, 0);
});

test("leaving via ✕ celebrates what the finish awarded", async () => {
  const t = await playing(3);
  await answerNow(t);
  let exited = false;
  void t.flow.exit().then(() => (exited = true));
  await settle();
  assert.deepEqual(t.calls("finishSession"), [["finishSession", 7]]);
  assert.equal(exited, false, "the caller navigates only once the finish is back");
  t.open.finish[0].resolve(summaryOf({ newBadges: [badge("ten-sessions")] }));
  await settle();
  assert.equal(exited, true);
  assert.deepEqual(t.seen.celebrated, [["ten-sessions"]]);
  // the navigation that follows tears the page down: no second finish
  t.flow.leave();
  await settle();
  assert.equal(t.calls("finishSession").length, 1);
  assert.equal(t.seen.dones.length, 0);
  assert.equal(t.view.phase, "playing");
  // nor does a second click on "Salir"
  await t.flow.exit();
  assert.equal(t.calls("finishSession").length, 1);
});

test("✕ right after an answer waits for its grade before finishing", async () => {
  const t = await playing(3);
  void t.flow.answer(false, "a");
  let exited = false;
  void t.flow.exit().then(() => (exited = true));
  await settle();
  assert.equal(t.calls("finishSession").length, 0);
  t.open.submit[0].resolve(feedback({ newBadges: [badge("combo-10")] }));
  await settle();
  assert.equal(t.calls("finishSession").length, 1);
  t.open.finish[0].resolve(summaryOf());
  await settle();
  assert.equal(exited, true);
  // the round's badge is announced even though the page no longer updates
  assert.deepEqual(t.seen.celebrated, [["combo-10"]]);
  assert.equal(t.view.xp, 0);
  assert.equal(t.clock.waiting(), 0);
});

test("a finish that fails on the way out is reported, and the exit still completes", async () => {
  const t = await playing(3);
  let exited = false;
  void t.flow.exit().then(() => (exited = true));
  await settle();
  t.open.finish[0].reject(new Error("no finish"));
  await settle();
  assert.equal(exited, true);
  assert.equal(t.seen.errors.length, 1);
});

test("Salir while the last round's finish is on its way doesn't finish the session twice", async () => {
  const t = await playing(1);
  await answerNow(t);
  t.clock.tick(700);
  assert.equal(t.calls("finishSession").length, 1);
  // ✕, then "Salir", before the finish is back
  t.flow.setPaused(true);
  let exited = false;
  void t.flow.exit().then(() => (exited = true));
  await settle();
  assert.equal(exited, true, "there is no finish of its own to wait for");
  assert.equal(t.calls("finishSession").length, 1);
  assert.deepEqual(t.seen.celebrated, []);
  t.open.finish[0].resolve(summaryOf({ newBadges: [badge("words-50")] }));
  await settle();
  // the summary that would have listed them never shows: a toast does
  assert.deepEqual(t.seen.celebrated, [["words-50"]]);
  assert.equal(t.view.phase, "playing");
  assert.equal(t.view.summary, null);
  assert.equal(t.seen.dones.length, 0);
  // the dialog closing and the page going change nothing more
  t.flow.setPaused(false);
  t.flow.leave();
  await settle();
  assert.equal(t.calls("finishSession").length, 1);
  assert.deepEqual(t.seen.celebrated, [["words-50"]]);
  assert.equal(t.view.phase, "playing");
});

test("leaving while a dictation miss is held finishes the session once", async () => {
  const t = await playing(3, { reveal: 650 });
  void t.flow.answer(false, "la frase", undefined, undefined, true);
  t.open.submit[0].resolve(feedback());
  await settle();
  t.clock.tick(3000);
  assert.equal(t.view.held, true);
  // Esc in the sidecar
  t.flow.leave();
  await settle();
  assert.deepEqual(t.calls("finishSession"), [["finishSession", 7]]);
  t.open.finish[0].resolve(summaryOf({ newBadges: [badge("sidecar-hour")] }));
  await settle();
  assert.deepEqual(t.seen.celebrated, [["sidecar-hour"]]);
  // an Enter on the dying page doesn't bring the next round up
  t.flow.continue();
  t.clock.tick(5000);
  assert.equal(t.view.idx, 0);
  assert.equal(t.view.held, true);
  assert.equal(t.view.phase, "playing");
  assert.equal(t.seen.dones.length, 0);
  assert.equal(t.clock.waiting(), 0);
  // the teardown that follows the navigation
  t.flow.leave();
  await settle();
  assert.equal(t.calls("finishSession").length, 1);
  assert.equal(t.calls("submitRound").length, 1);
});

// ---------- answering ----------

test("an answer is graded at once and the next round appears after the reveal", async () => {
  const t = await playing(3);
  t.clock.tick(4000);
  void t.flow.answer(false, "a", 2, [5, 6]);
  assert.deepEqual(t.calls("submitRound"), [
    ["submitRound", 7, 100, { roundIndex: 0, correct: false, durationMs: 4000, quality: 2, missedWordIds: [5, 6] }],
  ]);
  assert.equal(t.view.answering, true);
  t.open.submit[0].resolve(feedback({ xpGained: 12, combo: 4 }));
  await settle();
  // the score moves with the grade; the round stays up for the reveal
  assert.equal(t.view.xp, 12);
  assert.equal(t.view.combo, 4);
  assert.equal(t.view.idx, 0);
  assert.equal(t.view.flash, null);
  t.clock.tick(699);
  assert.equal(t.view.idx, 0);
  t.clock.tick(1);
  assert.equal(t.view.idx, 1);
  assert.equal(t.view.answering, false);
  assert.deepEqual(t.view.flash, { correct: false, text: "era a", seq: 1 });

  // the next round's time starts when it appears: the reveal isn't counted
  t.clock.tick(2500);
  void t.flow.answer(true);
  assert.equal(t.calls("submitRound")[1][3].durationMs, 2500);
  assert.equal(t.calls("submitRound")[1][3].roundIndex, 1);
  assert.equal(t.calls("submitRound")[1][2], 101);
  assert.equal(t.calls("submitRound")[1][3].quality, null);
  assert.deepEqual(t.calls("submitRound")[1][3].missedWordIds, []);
});

test("a slow submit eats into the reveal instead of adding to it", async () => {
  const t = await playing(3);
  void t.flow.answer(true, "a");
  t.clock.tick(500);
  t.open.submit[0].resolve(feedback());
  await settle();
  t.clock.tick(199);
  assert.equal(t.view.idx, 0);
  t.clock.tick(1);
  assert.equal(t.view.idx, 1);

  // slower than the reveal: the next round comes as soon as the grade is in
  void t.flow.answer(true, "a");
  t.clock.tick(3000);
  t.open.submit[1].resolve(feedback());
  await settle();
  assert.equal(t.view.idx, 2);
});

test("a round with nothing to reveal moves on with its grade", async () => {
  const t = await playing(3, { reveal: 0 });
  await answerNow(t);
  assert.equal(t.view.idx, 1);
  assert.equal(t.clock.waiting(), 1, "only the pill's timer");
});

test("a second answer to the same round is ignored", async () => {
  const t = await playing(3);
  void t.flow.answer(true, "a");
  void t.flow.answer(true, "a");
  assert.equal(t.calls("submitRound").length, 1);
  t.open.submit[0].resolve(feedback());
  await settle();
  // still the same round on screen during the reveal
  void t.flow.answer(false, "a");
  assert.equal(t.calls("submitRound").length, 1);
  t.clock.tick(700);
  assert.equal(t.view.idx, 1);
  assert.equal(t.view.xp, 10);
});

test("answers before the session is on screen are ignored", async () => {
  const t = setup();
  void t.flow.answer(true, "a");
  void t.flow.start();
  void t.flow.answer(true, "a");
  await settle();
  assert.equal(t.calls("submitRound").length, 0);
});

test("a failed submit doesn't leave the player stuck", async () => {
  const t = await playing(3);
  void t.flow.answer(true, "a");
  t.open.submit[0].reject(new Error("no grade"));
  await settle();
  assert.equal(t.seen.errors.length, 1);
  assert.equal(t.view.idx, 0);
  t.clock.tick(700);
  assert.equal(t.view.idx, 1);
  assert.equal(t.view.answering, false);
  // no grade, so no praise and no score
  assert.equal(t.view.flash, null);
  assert.equal(t.view.xp, 0);
  assert.equal(t.seen.feedbacks.length, 0);
  // and the next round can be answered
  await answerNow(t);
  assert.equal(t.calls("submitRound").length, 2);
});

test("the last answer finishes the session and shows the summary", async () => {
  const t = await playing(2);
  await answerNow(t);
  t.clock.tick(700);
  await answerNow(t);
  assert.equal(t.calls("finishSession").length, 0);
  t.clock.tick(700);
  assert.deepEqual(t.calls("finishSession"), [["finishSession", 7]]);
  assert.equal(t.view.phase, "playing");
  const s = summaryOf();
  t.open.finish[0].resolve(s);
  await settle();
  assert.equal(t.view.phase, "done");
  assert.deepEqual(t.view.summary, s);
  assert.deepEqual(t.seen.dones, [s]);
  // the round is over: nothing more is graded or finished
  void t.flow.answer(true, "a");
  t.flow.leave();
  await settle();
  assert.equal(t.calls("submitRound").length, 2);
  assert.equal(t.calls("finishSession").length, 1);
});

test("a failed finish shows the error state", async () => {
  const t = await playing(1);
  await answerNow(t);
  t.clock.tick(700);
  t.open.finish[0].reject(new Error("no summary"));
  await settle();
  assert.equal(t.view.phase, "error");
  assert.equal(t.seen.errors.length, 1);
  assert.equal(t.seen.dones.length, 0);
  // the session it played is still there for the error screen to tell apart
  assert.notEqual(t.view.session, null);
});

test("a sound or confetti that throws doesn't strand the session", async () => {
  const t = setup();
  // the same flow, but its page callbacks all blow up
  const view = initialView();
  const errors = [];
  const boom = () => {
    throw new Error("no audio device");
  };
  const flow = createFlow(
    view,
    {
      api: {
        getSettings: () => Promise.resolve({ definitionLang: "en", sessionLength: "standard" }),
        startSession: () => Promise.resolve(sessionOf(1)),
        submitRound: () => Promise.resolve(feedback({ newBadges: [badge("combo-10")] })),
        finishSession: () => Promise.resolve(summaryOf()),
      },
      clock: t.clock,
      celebrate: boom,
      reportError: (e) => errors.push(e),
      flashText: boom,
      onFeedback: boom,
      onDone: boom,
      revealMs: () => 700,
    },
    { requested: "quick" },
  );
  await flow.start();
  await flow.answer(true, "a");
  t.clock.tick(700);
  await settle();
  assert.equal(view.phase, "done");
  assert.equal(errors.length, 4);
});

// ---------- the pill ----------

test("a new pill gets its full time", async () => {
  const t = await playing(4, { reveal: 0 });
  await answerNow(t, false); // t=0: a miss, its pill is due down at 1400
  assert.deepEqual(t.view.flash, { correct: false, text: "era a", seq: 1 });
  t.clock.tick(300);
  await answerNow(t, true); // t=300: a correct pill, due down at 1200
  assert.deepEqual(t.view.flash, { correct: true, text: "¡Bien!", seq: 2 });
  assert.equal(t.clock.waiting(), 1, "the first pill's timer is gone, not left to fire later");
  t.clock.tick(600); // t=900
  assert.equal(t.view.flash.seq, 2);
  t.clock.tick(299); // t=1199
  assert.ok(t.view.flash, "still up just before its 900 ms are over");
  t.clock.tick(1); // t=1200
  assert.equal(t.view.flash, null);

  // and the first pill's timer is gone rather than waiting to take down a later one
  await answerNow(t, true); // t=1200, seq 3, due down at 2100
  t.clock.tick(200); // t=1400: where the miss's timer would have fired
  assert.equal(t.view.flash.seq, 3);
  t.clock.tick(700);
  assert.equal(t.view.flash, null);
});

test("the pill says what the page tells it to", async () => {
  const t = await playing(3, { reveal: 0 });
  await answerNow(t, false, feedback(), "la casa");
  assert.equal(t.view.flash.text, "era la casa");
  assert.equal(t.view.flash.correct, false);
});

// ---------- badges ----------

test("round badges are celebrated at once and kept for the summary", async () => {
  const t = await playing(2);
  await answerNow(t, true, feedback({ newBadges: [badge("combo-10")] }));
  assert.deepEqual(t.seen.celebrated, [["combo-10"]]);
  assert.deepEqual(
    t.view.roundBadges.map((b) => b.id),
    ["combo-10"],
  );
  t.clock.tick(700);
  // a round with no badge announces nothing
  await answerNow(t);
  assert.equal(t.seen.celebrated.length, 1);
  t.clock.tick(700);
  t.open.finish[0].resolve(summaryOf({ newBadges: [badge("combo-10"), badge("words-50")] }));
  await settle();
  // the summary screen lists them all, each once; its own are not toasted again
  assert.deepEqual(
    mergeBadges(t.view.roundBadges, t.view.summary.newBadges).map((b) => b.id),
    ["combo-10", "words-50"],
  );
  assert.equal(t.seen.celebrated.length, 1);
});

test("mergeBadges keeps the first of each, in order", () => {
  const a = badge("a");
  const again = { ...badge("a"), name: "later copy" };
  assert.deepEqual(mergeBadges([a, badge("b")], [again, badge("c")], []), [a, badge("b"), badge("c")]);
  assert.deepEqual(mergeBadges(), []);
});

// ---------- the exit dialog ----------

test("the next round waits while the exit dialog is open", async () => {
  const t = await playing(3);
  t.flow.setPaused(true);
  await answerNow(t);
  t.clock.tick(2000);
  assert.equal(t.view.idx, 0);
  assert.equal(t.view.flash, null);
  // answering again behind the dialog does nothing
  void t.flow.answer(true, "a");
  assert.equal(t.calls("submitRound").length, 1);
  t.flow.setPaused(false);
  assert.equal(t.view.idx, 1);
  assert.equal(t.view.flash.seq, 1);
  // closing a dialog with nothing waiting changes nothing
  t.flow.setPaused(true);
  t.flow.setPaused(false);
  assert.equal(t.view.idx, 1);
});

test("the session doesn't finish under the exit dialog", async () => {
  const t = await playing(1);
  await answerNow(t);
  t.flow.setPaused(true);
  t.clock.tick(2000);
  assert.equal(t.calls("finishSession").length, 0);
  t.flow.setPaused(false);
  assert.equal(t.calls("finishSession").length, 1);
});

test("a dialog opened and closed within the reveal doesn't cut it short", async () => {
  const t = await playing(3);
  await answerNow(t);
  t.flow.setPaused(true);
  t.clock.tick(300);
  t.flow.setPaused(false);
  assert.equal(t.view.idx, 0);
  t.clock.tick(400);
  assert.equal(t.view.idx, 1);
});

/** The last round answered and revealed: finishSession is on its way, and ✕ is still on screen. */
async function finishing() {
  const t = await playing(1);
  await answerNow(t);
  t.clock.tick(700);
  assert.equal(t.calls("finishSession").length, 1);
  assert.equal(t.view.phase, "playing");
  return t;
}

test("a summary that arrives under the exit dialog waits for it to close", async () => {
  const t = await finishing();
  t.flow.setPaused(true);
  const s = summaryOf({ newBadges: [badge("words-50")] });
  t.open.finish[0].resolve(s);
  await settle();
  assert.equal(t.view.phase, "playing", "the screen doesn't change under the dialog");
  assert.equal(t.view.summary, null);
  assert.equal(t.seen.dones.length, 0);
  // "Seguir jugando"
  t.flow.setPaused(false);
  assert.equal(t.view.phase, "done");
  assert.deepEqual(t.view.summary, s);
  assert.deepEqual(t.seen.dones, [s]);
  // the summary lists its badges itself: no toast
  assert.deepEqual(t.seen.celebrated, []);
  // and it is shown once
  t.flow.setPaused(true);
  t.flow.setPaused(false);
  assert.equal(t.seen.dones.length, 1);
  assert.equal(t.calls("finishSession").length, 1);
});

test("a dialog closed again before the finish is back holds nothing up", async () => {
  const t = await finishing();
  t.flow.setPaused(true);
  t.flow.setPaused(false);
  assert.equal(t.view.phase, "playing");
  const s = summaryOf();
  t.open.finish[0].resolve(s);
  await settle();
  assert.equal(t.view.phase, "done");
  assert.deepEqual(t.seen.dones, [s]);
});

test("Salir on a session that finished under the exit dialog announces its badges", async () => {
  const t = await finishing();
  t.flow.setPaused(true);
  t.open.finish[0].resolve(summaryOf({ newBadges: [badge("words-50")] }));
  await settle();
  assert.deepEqual(t.seen.celebrated, [], "not yet: the summary may still get to list them");
  let exited = false;
  void t.flow.exit().then(() => (exited = true));
  await settle();
  assert.equal(exited, true);
  assert.deepEqual(t.seen.celebrated, [["words-50"]]);
  // the dialog closing as the page goes brings no summary up
  t.flow.setPaused(false);
  t.flow.leave();
  await settle();
  assert.equal(t.view.phase, "playing");
  assert.equal(t.view.summary, null);
  assert.equal(t.seen.dones.length, 0);
  assert.deepEqual(t.seen.celebrated, [["words-50"]]);
  assert.equal(t.calls("finishSession").length, 1);
});

test("a page torn down with its summary still behind the dialog announces the badges once", async () => {
  const t = await finishing();
  t.flow.setPaused(true);
  t.open.finish[0].resolve(summaryOf({ newBadges: [badge("words-50")] }));
  await settle();
  t.flow.leave();
  t.flow.leave();
  t.flow.setPaused(false);
  await settle();
  assert.deepEqual(t.seen.celebrated, [["words-50"]]);
  assert.equal(t.view.phase, "playing");
  assert.equal(t.seen.dones.length, 0);
  assert.equal(t.calls("finishSession").length, 1);
});

test("a finish that fails under the exit dialog shows the error state when it closes", async () => {
  const t = await finishing();
  t.flow.setPaused(true);
  t.open.finish[0].reject(new Error("no summary"));
  await settle();
  assert.equal(t.seen.errors.length, 1, "said at once: the error toast sits above the dialog");
  assert.equal(t.view.phase, "playing");
  t.flow.setPaused(false);
  assert.equal(t.view.phase, "error");
  assert.equal(t.seen.errors.length, 1);
  assert.equal(t.seen.dones.length, 0);
  assert.equal(t.view.summary, null);
});

// ---------- holding (sidecar dictation) ----------

test("a dictation miss waits for Enter", async () => {
  const t = await playing(3, { reveal: 650 });
  void t.flow.answer(false, "la frase", undefined, undefined, true);
  t.open.submit[0].resolve(feedback());
  await settle();
  t.clock.tick(5000);
  assert.equal(t.view.idx, 0);
  assert.equal(t.view.held, true);
  // the held round can't be answered a second time
  void t.flow.answer(true, "la frase");
  assert.equal(t.calls("submitRound").length, 1);
  t.flow.continue();
  assert.equal(t.view.held, false);
  assert.equal(t.view.idx, 1);
  assert.equal(t.view.answering, false);
  // Enter again is not a second continue
  t.flow.continue();
  assert.equal(t.view.idx, 1);
});

test("an exact dictation answer moves on by itself", async () => {
  const t = await playing(3, { reveal: 650 });
  void t.flow.answer(true, "la frase", undefined, undefined, false);
  t.open.submit[0].resolve(feedback());
  await settle();
  assert.equal(t.view.held, false);
  t.clock.tick(650);
  assert.equal(t.view.idx, 1);
});

test("a held last round finishes on Enter", async () => {
  const t = await playing(1, { reveal: 650 });
  void t.flow.answer(false, "la frase", undefined, undefined, true);
  t.open.submit[0].resolve(feedback());
  await settle();
  assert.equal(t.calls("finishSession").length, 0);
  t.flow.continue();
  assert.equal(t.calls("finishSession").length, 1);
});

test("a miss whose grade failed still waits for Enter", async () => {
  const t = await playing(2, { reveal: 650 });
  void t.flow.answer(false, "la frase", undefined, undefined, true);
  t.open.submit[0].reject(new Error("no grade"));
  await settle();
  assert.equal(t.seen.errors.length, 1);
  assert.equal(t.view.held, true);
  t.flow.continue();
  assert.equal(t.view.idx, 1);
});

// ---------- a fresh instance ----------

test("a fresh flow starts clean", async () => {
  const first = await playing(2, { reveal: 0 });
  await answerNow(first, false, feedback({ xpGained: 25, combo: 3, newBadges: [badge("combo-10")] }));
  assert.equal(first.view.idx, 1);
  assert.ok(first.view.flash);

  const second = await playing(2);
  assert.equal(second.view.idx, 0);
  assert.equal(second.view.xp, 0);
  assert.equal(second.view.combo, 0);
  assert.equal(second.view.flash, null);
  assert.deepEqual(second.view.roundBadges, []);
  assert.equal(second.view.answering, false);
  assert.equal(second.view.summary, null);
  // and it accepts answers, whatever state the first was left in
  await answerNow(second);
  assert.equal(second.calls("submitRound").length, 1);
  assert.notEqual(initialView(), initialView());
  assert.notEqual(initialView().roundBadges, initialView().roundBadges);
});

// ---------- the reveal table ----------

test("every round type has its reveal time", () => {
  assert.deepEqual(PLAY_REVEAL_MS, {
    new_word: 0,
    review_card: 0,
    choice: 700,
    cloze: 700,
    listen: 700,
    match: 600,
    build: 900,
    conjugation: 900,
    listen_type: 900,
  });
});
