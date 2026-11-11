// Wording helpers for the UI's Spanish. No `$lib` imports, so `node --test`
// can load this file as it is.

/** The noun a count takes: the singular only for one (zero takes the plural). */
function nounFor(n: number, one: string, many: string): string {
  return n === 1 ? one : many;
}

/** A count with its noun: "1 palabra", "2 palabras", and "0 palabras". */
export function plural(n: number, one: string, many: string): string {
  return `${n} ${nounFor(n, one, many)}`;
}

/**
 * A session's length as its summary shows it, the number apart from its unit
 * (they are set on two lines). Whole minutes, rounded down as Home's total
 * is: rounded to the nearest, the summary said 3 where Home then added 2.
 * Less than a minute is "<1", not 0.
 */
export function sessionMinutes(minutes: number): { count: string; unit: string } {
  if (minutes < 1) return { count: "<1", unit: "minuto" };
  const n = Math.floor(minutes);
  return { count: String(n), unit: nounFor(n, "minuto", "minutos") };
}

/**
 * What a summary's accuracy ring says, and how much of it is filled (0 to 1;
 * null draws no arc at all). Intro cards are not answers, so a session can
 * end without one: that is "no answers", a dash in an empty ring, and not 0%
 * right.
 */
export function accuracyRing(rounds: number, accuracy: number): { label: string; fill: number | null } {
  if (rounds === 0) return { label: "—", fill: null };
  return { label: `${Math.round(accuracy * 100)}%`, fill: Math.min(1, Math.max(0, accuracy)) };
}

/** When a card comes up again, from days as a fraction: "ya", "en 5 h", "en 2 días", "en 6 meses". */
export function dueIn(days: number): string {
  if (days < 1 / 24) return "ya";
  if (days < 1) return `en ${Math.max(1, Math.round(days * 24))} h`;
  if (days < 30) return `en ${plural(Math.max(1, Math.round(days)), "día", "días")}`;
  if (days < 365) return `en ${plural(Math.round(days / 30), "mes", "meses")}`;
  return `en ${plural(Math.round(days / 365), "año", "años")}`;
}
