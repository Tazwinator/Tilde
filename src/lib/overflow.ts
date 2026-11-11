/**
 * A box that scrolls inside the page: whether it holds more than it shows,
 * and whether something is still below its lower edge. No `$lib` imports, so
 * `node --test` can load this file as it is.
 */

/** A scrolling box's measures, as an element has them. */
export interface ScrollBox {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}

export interface Overflow {
  /** it holds more than it shows, wherever it is scrolled to */
  scrolls: boolean;
  /** some of what it holds is below its lower edge right now */
  moreBelow: boolean;
}

/**
 * `paddingBottom` is the box's own padding under its last child: when that
 * is all that is out of view, nothing is hidden. A pixel doesn't count
 * either: the sizes are whole numbers but `scrollTop` is not, so a box
 * scrolled all the way can stop a fraction short of the end.
 */
export function overflow(box: ScrollBox, paddingBottom = 0): Overflow {
  const hidden = box.scrollHeight - box.clientHeight;
  return {
    scrolls: hidden > 1,
    moreBelow: hidden - box.scrollTop - paddingBottom > 1,
  };
}

/**
 * True when a box that doesn't fit should first be laid out tighter: once.
 * A box already packed that still doesn't fit is left to scroll. No measure
 * says "unpack": the box would then not fit, be packed again, and flicker
 * between the two. Whoever packs it decides when to try again (the session
 * summary does when the window changes size).
 */
export function needsPacking(fit: Overflow, packed: boolean): boolean {
  return fit.scrolls && !packed;
}

/**
 * Tells `report` how the box stands now, and again whenever that may have
 * changed: when it is scrolled, and when it or one of its children changes
 * size (the window was resized, a font arrived). Returns the way to stop.
 *
 * A change of size is reported on the next frame, not from inside the
 * observer: `report` may change the layout, and a layout changed while the
 * browser is handing out size changes makes it give up on the rest ("loop
 * completed with undelivered notifications").
 */
export function watchOverflow(node: HTMLElement, report: (state: Overflow) => void): () => void {
  const measure = () => report(overflow(node, parseFloat(getComputedStyle(node).paddingBottom) || 0));
  let frame: number | null = null;
  const sizes = new ResizeObserver(() => {
    if (frame !== null) return;
    frame = requestAnimationFrame(() => {
      frame = null;
      measure();
    });
  });
  sizes.observe(node);
  for (const child of node.children) sizes.observe(child);
  node.addEventListener("scroll", measure, { passive: true });
  measure();
  return () => {
    sizes.disconnect();
    if (frame !== null) cancelAnimationFrame(frame);
    node.removeEventListener("scroll", measure);
  };
}
