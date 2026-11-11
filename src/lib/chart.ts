import type { SeriesPoint } from "./contract";

// Where the stats charts put their points and labels, in the units of their
// SVG viewBox (320 wide; labels in font-size 10). Kept out of the page so it
// can be tested without a browser.

const MESES = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];

function ymd(iso: string): number[] {
  return iso.split("-").map(Number);
}

/** A `YYYY-MM-DD` date as days since 1970. Not `new Date(iso)`: that reads
 * the date as midnight UTC, which is the evening before west of Greenwich. */
export function parseDay(iso: string): number {
  const [y, m, d] = ymd(iso);
  return Date.UTC(y, m - 1, d) / 86_400_000;
}

/** "5 oct", or "5 oct 26" where the year is needed to tell dates apart. */
export function dayLabel(iso: string, withYear: boolean): string {
  const [y, m, d] = ymd(iso);
  const day = `${d} ${MESES[m - 1]}`;
  return withYear ? `${day} ${String(y % 100).padStart(2, "0")}` : day;
}

/** The middle of each of `n` equal columns: where a weekly chart puts a bar, a point and its label. */
export function columnXs(n: number, w = 320): number[] {
  return Array.from({ length: n }, (_, i) => ((i + 0.5) * w) / n);
}

/** The height of each point on a scale from 0 to the largest value. */
export function scaleYs(points: SeriesPoint[], h = 132, pad = 8): number[] {
  const max = Math.max(...points.map((p) => p.value), 1);
  return points.map((p) => h - pad - (p.value / max) * (h - pad * 2));
}

export function linePath(xs: number[], ys: number[]): string {
  return xs.map((x, i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${ys[i].toFixed(1)}`).join(" ");
}

/** The line, closed down to `floor`, for the tint under it. */
export function areaPath(xs: number[], ys: number[], floor: number): string {
  if (xs.length === 0) return "";
  return `${linePath(xs, ys)} L${xs[xs.length - 1].toFixed(1)},${floor} L${xs[0].toFixed(1)},${floor} Z`;
}

export interface Tick {
  x: number;
  label: string;
  anchor: "start" | "middle" | "end";
}

export interface WordsChart {
  /** one x per point */
  xs: number[];
  /** the dates worth writing under the chart */
  ticks: Tick[];
  withYear: boolean;
}

// Labels are 10 units tall; a character of one is about half that wide, and
// 5.5 leaves a little air. Two labels also never sit closer than MIN_STEP.
const CHAR_W = 5.5;
const LABEL_GAP = 4;
const MIN_STEP = 50;

function reach(t: Tick): [number, number] {
  const width = t.label.length * CHAR_W;
  const left = t.anchor === "start" ? t.x : t.anchor === "end" ? t.x - width : t.x - width / 2;
  return [left, left + width];
}

function clear(a: Tick, b: Tick): boolean {
  return b.x - a.x >= MIN_STEP && reach(b)[0] - reach(a)[1] >= LABEL_GAP;
}

/**
 * The "Palabras que conoces" chart: one point per day played, labelled with
 * its date (`YYYY-MM-DD`, oldest first). Points are placed by date, not by
 * position in the list, so a month away looks like a month. At most five
 * dates are written, spread over the time covered.
 */
export function wordsChart(points: SeriesPoint[], w = 320, pad = 8): WordsChart {
  const n = points.length;
  if (n === 0) return { xs: [], ticks: [], withYear: false };
  if (n === 1) {
    return { xs: [w / 2], ticks: [{ x: w / 2, label: dayLabel(points[0].label, false), anchor: "middle" }], withYear: false };
  }
  const days = points.map((p) => parseDay(p.label));
  const first = days[0];
  const span = days[n - 1] - first;
  const xs = days.map((d) => (span > 0 ? pad + ((d - first) / span) * (w - pad * 2) : w / 2));
  const withYear = points[0].label.slice(0, 4) !== points[n - 1].label.slice(0, 4);

  // five dates evenly spread over the span; for each, the point nearest to it
  const picked = new Set<number>();
  for (let k = 0; k <= 4; k++) {
    const target = first + (k * span) / 4;
    let nearest = 0;
    for (let i = 1; i < n; i++) {
      if (Math.abs(days[i] - target) < Math.abs(days[nearest] - target)) nearest = i;
    }
    picked.add(nearest);
  }

  const ticks: Tick[] = [];
  for (const i of [...picked].sort((a, b) => a - b)) {
    // the two ends hang inward, so their labels stay inside the chart
    const t: Tick = { x: xs[i], label: dayLabel(points[i].label, withYear), anchor: i === 0 ? "start" : i === n - 1 ? "end" : "middle" };
    if (i === n - 1) {
      // the newest point always keeps its date; one in its way gives way
      while (ticks.length > 1 && !clear(ticks[ticks.length - 1], t)) ticks.pop();
      ticks.push(t);
    } else if (ticks.length === 0 || clear(ticks[ticks.length - 1], t)) {
      ticks.push(t);
    }
  }
  return { xs, ticks, withYear };
}
