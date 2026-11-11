// Wording helpers for the UI's Spanish. No `$lib` imports, so `node --test`
// can load this file as it is.

/** A count with its noun: "1 palabra", "2 palabras", and "0 palabras" (zero takes the plural). */
export function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** When a card comes up again, from days as a fraction: "ya", "en 5 h", "en 2 días", "en 6 meses". */
export function dueIn(days: number): string {
  if (days < 1 / 24) return "ya";
  if (days < 1) return `en ${Math.max(1, Math.round(days * 24))} h`;
  if (days < 30) return `en ${plural(Math.max(1, Math.round(days)), "día", "días")}`;
  if (days < 365) return `en ${plural(Math.round(days / 30), "mes", "meses")}`;
  return `en ${plural(Math.round(days / 365), "año", "años")}`;
}
