// @ts-nocheck -- no @types/node in this project; Node strips the types when running it
import { test } from "node:test";
import assert from "node:assert/strict";
import { needsPacking, overflow, watchOverflow } from "../src/lib/overflow.ts";

const box = (scrollHeight, clientHeight, scrollTop = 0) => ({ scrollHeight, clientHeight, scrollTop });

test("a box that shows all it holds neither scrolls nor has more below", () => {
  assert.deepEqual(overflow(box(400, 400)), { scrolls: false, moreBelow: false });
  // sizes are rounded: a pixel is no overflow
  assert.deepEqual(overflow(box(401, 400)), { scrolls: false, moreBelow: false });
});

test("a box that holds more than it shows has more below until it is scrolled to the end", () => {
  assert.deepEqual(overflow(box(520, 440)), { scrolls: true, moreBelow: true });
  assert.deepEqual(overflow(box(520, 440, 40)), { scrolls: true, moreBelow: true });
  assert.deepEqual(overflow(box(520, 440, 80)), { scrolls: true, moreBelow: false });
  // scrollTop is not a whole number: all the way down can be a fraction short
  assert.deepEqual(overflow(box(520, 440, 79.4)), { scrolls: true, moreBelow: false });
  assert.deepEqual(overflow(box(520, 440, 78)), { scrolls: true, moreBelow: true });
});

test("the box's own bottom padding is nothing more to see", () => {
  // 20 px too tall with 20 px of padding under the last child: all of it is in view
  assert.deepEqual(overflow(box(460, 440), 20), { scrolls: true, moreBelow: false });
  assert.deepEqual(overflow(box(472, 440), 32), { scrolls: true, moreBelow: false });
  // 12 px of the last child are cut
  assert.deepEqual(overflow(box(472, 440), 20), { scrolls: true, moreBelow: true });
  // scrolled until only the padding is left below
  assert.deepEqual(overflow(box(520, 440, 59), 20), { scrolls: true, moreBelow: false });
  assert.deepEqual(overflow(box(520, 440, 50), 20), { scrolls: true, moreBelow: true });
});

test("a box that doesn't fit is packed once, then left to scroll", () => {
  const fits = { scrolls: false, moreBelow: false };
  const tooMuch = { scrolls: true, moreBelow: true };
  assert.equal(needsPacking(tooMuch, false), true);
  assert.equal(needsPacking(tooMuch, true), false);
  assert.equal(needsPacking(fits, false), false);
  // no measure unpacks it: that it fits once packed says nothing about fitting unpacked
  assert.equal(needsPacking(fits, true), false);
  // only its padding is out of view: still more than it has room for
  assert.equal(needsPacking({ scrolls: true, moreBelow: false }, false), true);
});

// the few browser pieces watchOverflow uses, as fakes
function fakeBrowser(t) {
  const frames = new Map();
  let nextFrame = 0;
  const observers = [];
  const before = {};
  const fakes = {
    getComputedStyle: (node) => ({ paddingBottom: node.paddingBottom ?? "" }),
    requestAnimationFrame: (fn) => {
      frames.set(++nextFrame, fn);
      return nextFrame;
    },
    cancelAnimationFrame: (id) => frames.delete(id),
    ResizeObserver: class {
      constructor(callback) {
        this.callback = callback;
        this.targets = [];
        this.connected = true;
        observers.push(this);
      }
      observe(target) {
        this.targets.push(target);
      }
      disconnect() {
        this.connected = false;
      }
    },
  };
  for (const [name, fake] of Object.entries(fakes)) {
    before[name] = globalThis[name];
    globalThis[name] = fake;
  }
  t.after(() => {
    for (const name of Object.keys(fakes)) globalThis[name] = before[name];
  });
  return {
    observers,
    pendingFrames: () => frames.size,
    /** the browser paints: every callback asked for runs once */
    frame() {
      const due = [...frames.values()];
      frames.clear();
      for (const fn of due) fn();
    },
  };
}

function fakeNode(measures, children = []) {
  const listeners = [];
  return {
    ...measures,
    children,
    listeners,
    addEventListener: (type, fn) => listeners.push([type, fn]),
    removeEventListener: (type, fn) => {
      const i = listeners.findIndex((l) => l[0] === type && l[1] === fn);
      if (i !== -1) listeners.splice(i, 1);
    },
    scroll(to) {
      this.scrollTop = to;
      for (const [type, fn] of [...listeners]) if (type === "scroll") fn();
    },
  };
}

test("the watch reports at once, on every scroll, and a frame after a change of size", (t) => {
  const browser = fakeBrowser(t);
  const kids = [{}, {}];
  const node = fakeNode({ scrollHeight: 520, clientHeight: 440, scrollTop: 0, paddingBottom: "20px" }, kids);
  const seen = [];
  const stop = watchOverflow(node, (fit) => seen.push(fit));

  // at once: the page lays itself out on this before the first paint
  assert.deepEqual(seen, [{ scrolls: true, moreBelow: true }]);
  assert.equal(browser.observers.length, 1);
  assert.deepEqual(browser.observers[0].targets, [node, ...kids]);

  // scrolled until only the padding is left: nothing more below
  node.scroll(60);
  assert.deepEqual(seen.at(-1), { scrolls: true, moreBelow: false });
  assert.equal(seen.length, 2);

  // The window grew. Not reported from inside the observer (the report may
  // change the layout), and once for however many boxes changed.
  node.clientHeight = 520;
  node.scrollTop = 0;
  browser.observers[0].callback();
  browser.observers[0].callback();
  assert.equal(seen.length, 2);
  assert.equal(browser.pendingFrames(), 1);
  browser.frame();
  assert.deepEqual(seen.at(-1), { scrolls: false, moreBelow: false });
  assert.equal(seen.length, 3);

  // and again after that frame
  node.clientHeight = 440;
  browser.observers[0].callback();
  browser.frame();
  assert.deepEqual(seen.at(-1), { scrolls: true, moreBelow: true });
  assert.equal(seen.length, 4);

  stop();
});

test("a stopped watch reports nothing more, not even the frame it had asked for", (t) => {
  const browser = fakeBrowser(t);
  const node = fakeNode({ scrollHeight: 520, clientHeight: 440, scrollTop: 0 });
  const seen = [];
  const stop = watchOverflow(node, (fit) => seen.push(fit));
  // no padding to read (an empty string): none is taken off
  assert.deepEqual(seen, [{ scrolls: true, moreBelow: true }]);

  browser.observers[0].callback();
  assert.equal(browser.pendingFrames(), 1);
  stop();
  assert.equal(browser.observers[0].connected, false);
  assert.equal(browser.pendingFrames(), 0);
  assert.deepEqual(node.listeners, []);
  browser.frame();
  node.scroll(80);
  assert.equal(seen.length, 1);
});
