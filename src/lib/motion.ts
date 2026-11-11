import { prefersReducedMotion } from "svelte/motion";

/** A transition's duration, or 0 when the player asked for less motion. The
 * reduced-motion rule in app.css only reaches CSS animations, not Svelte's. */
export const motion = (ms: number) => (prefersReducedMotion.current ? 0 : ms);
