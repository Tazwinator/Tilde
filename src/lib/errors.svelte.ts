/**
 * The one error the app is currently showing. Pages that can recover handle
 * their own failures; anything else (an unhandled rejection from a call that
 * nobody caught) lands here and the layout shows it with a way out.
 */
export const failure = $state<{ message: string | null }>({ message: null });

export function describe(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export function reportError(e: unknown): void {
  failure.message = describe(e);
}
