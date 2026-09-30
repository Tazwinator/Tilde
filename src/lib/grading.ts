/** Normalise a typed answer for comparison. */
export function norm(s: string): string {
  return s
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[.,!?¿¡;:]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}
