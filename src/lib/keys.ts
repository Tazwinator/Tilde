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

/**
 * The option a digit key picks (0-based), or null if there is no such option.
 * A round can come with fewer than four options, and then "4" picks nothing
 * rather than an answer that isn't on screen. Four is the most any round shows.
 */
export function optionFromKey(key: string, count: number): number | null {
  if (!/^[1-9]$/.test(key)) return null;
  const n = Number(key);
  return n <= count && n <= 4 ? n - 1 : null;
}

/**
 * What a key press chooses on a question that has, apart from its numbered
 * options, one more choice on a key of its own (`other`: the placement quiz's
 * "No lo sé"): the option's index, `other.choice`, or null for nothing. A key
 * still held down from the question before chooses nothing (held, it would
 * answer every question left), and neither does a shortcut: Ctrl+0 is not 0.
 */
export function choiceFromKey(
  e: Pick<KeyboardEvent, "key" | "repeat" | "ctrlKey" | "altKey" | "metaKey">,
  count: number,
  other: { readonly key: string; readonly choice: number },
): number | null {
  if (e.repeat || e.ctrlKey || e.altKey || e.metaKey) return null;
  if (e.key === other.key) return other.choice;
  return optionFromKey(e.key, count);
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
