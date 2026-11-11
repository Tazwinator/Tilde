/**
 * True when a key press is headed for a text field the player is typing in.
 * Window-level shortcuts (Space to replay, digits to pick) must leave those
 * keys alone, or a dictation answer can't contain a space.
 */
export function typingInto(e: KeyboardEvent): boolean {
  const t = e.target;
  return (
    t instanceof HTMLInputElement ||
    t instanceof HTMLTextAreaElement ||
    (t instanceof HTMLElement && t.isContentEditable)
  );
}

type Press = Pick<KeyboardEvent, "key" | "code" | "repeat" | "ctrlKey" | "altKey" | "metaKey" | "target">;

/**
 * True when Enter or Space, pressed anywhere on the page, should do the
 * screen's main action. Not a key held down from the screen before (it would
 * skip a card nobody read), not a shortcut with Ctrl/Alt/Meta, and not a
 * press that belongs to the control it lands on: a focused button clicks
 * itself, a text field takes the character.
 *
 * It goes by `tagName` rather than `instanceof`, so it runs without a DOM.
 */
export function activates(e: Press): boolean {
  if (e.key !== "Enter" && e.key !== " " && e.code !== "Space") return false;
  if (e.repeat || e.ctrlKey || e.altKey || e.metaKey) return false;
  const t = e.target as { tagName?: string; isContentEditable?: boolean } | null;
  if (!t) return true;
  if (t.isContentEditable) return false;
  return !["BUTTON", "A", "INPUT", "TEXTAREA", "SELECT"].includes((t.tagName ?? "").toUpperCase());
}
