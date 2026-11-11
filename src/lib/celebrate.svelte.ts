import type { Badge } from "./contract";
import { createBadgeQueue, SOUND_GAP_MS, spaced, type BadgeToast } from "./badgeQueue";
import { sfx } from "./sfx";

/**
 * The badge toasts on screen right now. The layout renders them on every
 * page, so a badge is announced wherever the player is when it arrives: in
 * the middle of a round, or on Home after leaving a session through ✕.
 */
export const toasts = $state<BadgeToast[]>([]);

/**
 * The badge sound, for a toast and for a screen that lists its badges itself
 * (a summary). Both go through this one gate: a summary that comes right
 * after a toast doesn't sound a second time.
 */
export const badgeSound = spaced(
  () => sfx.badge(),
  SOUND_GAP_MS,
  () => performance.now(),
);

// timers live here, not in a component: the layout never unmounts
const queue = createBadgeQueue(toasts, {
  setTimeout: (fn, ms) => setTimeout(fn, ms),
  clearTimeout: (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
  onNew: badgeSound,
});

/** Announces newly earned badges: a toast each, one sound for the lot. */
export function celebrate(badges: Badge[]): void {
  queue.add(badges);
}

export function dismissToast(id: number): void {
  queue.dismiss(id);
}
