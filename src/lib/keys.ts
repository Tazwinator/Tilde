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
