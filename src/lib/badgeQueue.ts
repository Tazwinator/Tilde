/**
 * The queue behind the badge toasts (see celebrate.svelte.ts): which badges
 * are showing, which wait their turn, and when each goes away; and the gate
 * that keeps their sound from repeating. It works on a list handed in from
 * outside (a `$state` proxy in the app, a plain array in the tests) and has
 * no runtime imports, so `node --test` can load it.
 */
import type { Badge } from "./contract";

export interface BadgeToast {
  /** unique per toast, so a badge announced again later is a new entry */
  id: number;
  badge: Badge;
}

export const TOAST_MS = 4000;
export const MAX_TOASTS = 3;

/**
 * Badges this close together are one moment to the ear: the toast for a
 * badge from the last round and, a reveal later, the summary with its own.
 * Longer than the longest reveal plus an ordinary finish.
 */
export const SOUND_GAP_MS = 1500;

/** `play`, except that it stays silent within `gapMs` of the last time it sounded. */
export function spaced(play: () => void, gapMs: number, now: () => number): () => void {
  let last = -Infinity;
  return () => {
    const t = now();
    if (t - last < gapMs) return;
    last = t;
    play();
  };
}

export interface BadgeQueueDeps {
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(id: unknown): void;
  /** Called once per add() that queued at least one badge: the sound. */
  onNew(): void;
}

export function createBadgeQueue(showing: BadgeToast[], deps: BadgeQueueDeps) {
  const waiting: Badge[] = [];
  const timers = new Map<number, unknown>();
  let nextId = 1;

  function fill(): void {
    while (showing.length < MAX_TOASTS && waiting.length > 0) {
      const badge = waiting.shift() as Badge;
      const id = nextId++;
      showing.push({ id, badge });
      // the time starts when the toast appears, not when it was queued
      timers.set(
        id,
        deps.setTimeout(() => dismiss(id), TOAST_MS),
      );
    }
  }

  /** Announces each badge that isn't already showing or waiting. */
  function add(badges: Badge[]): void {
    let queued = false;
    for (const badge of badges) {
      // the same badge can arrive twice: with a round's feedback and again
      // from the finish of a session the player has just left
      if (showing.some((t) => t.badge.id === badge.id) || waiting.some((b) => b.id === badge.id)) continue;
      waiting.push(badge);
      queued = true;
    }
    if (!queued) return;
    deps.onNew();
    fill();
  }

  /** Takes a toast down (its time is up, or it was clicked) and lets the next one in. */
  function dismiss(id: number): void {
    if (timers.has(id)) {
      deps.clearTimeout(timers.get(id));
      timers.delete(id);
    }
    const at = showing.findIndex((t) => t.id === id);
    if (at === -1) return;
    showing.splice(at, 1);
    fill();
  }

  return { add, dismiss };
}
