/**
 * One session from start to summary, with no framework in it. The page hands
 * in a view object (a `$state` proxy, so what is written here shows on
 * screen) and every side effect; `node --test` drives the same code with a
 * fake clock. Play and the sidecar both run on it.
 *
 * It must stay loadable by `node --test` as it is: no runtime imports, and
 * only TypeScript that can be stripped (no enums, no parameter properties).
 */
import type { Badge, Round, RoundFeedback, RoundResult, SessionKind, SessionStart, SessionSummary } from "./contract";

/** The kinds the play page can run. */
export type PlayKind = "quick" | "standard" | "deep" | "review_only";

const LENGTHS: readonly string[] = ["quick", "standard", "deep"];

function isPlayKind(kind: string | null | undefined): kind is PlayKind {
  return kind === "review_only" || (kind != null && LENGTHS.includes(kind));
}

/**
 * The session a visit to /play starts: what the URL asks for when that is a
 * kind the page can run, else the saved "sesión por defecto", else Normal.
 * Neither value is checked anywhere before it gets here.
 */
export function resolveKind(requested: string | null, saved: string | null | undefined): PlayKind {
  if (isPlayKind(requested)) return requested;
  if (saved != null && LENGTHS.includes(saved)) return saved as PlayKind;
  return "standard";
}

/** The lists joined in order, each badge once. */
export function mergeBadges(...lists: Badge[][]): Badge[] {
  const seen = new Set<string>();
  const merged: Badge[] = [];
  for (const list of lists) {
    for (const badge of list) {
      if (seen.has(badge.id)) continue;
      seen.add(badge.id);
      merged.push(badge);
    }
  }
  return merged;
}

/**
 * How long an answered round stays up before the next one appears. An intro
 * card and a self-graded card have nothing to reveal.
 */
export const PLAY_REVEAL_MS: Record<Round["type"], number> = {
  new_word: 0,
  review_card: 0,
  choice: 700,
  cloze: 700,
  listen: 700,
  match: 600,
  build: 900,
  conjugation: 900,
  listen_type: 900,
};

export const SIDECAR_REVEAL_MS = 650;

/**
 * The `quality` an intro card sends for "Ya me la sé": round_submit then
 * saves the word as known instead of starting to teach it (see
 * RoundResult.quality). "¡Lo tengo!" sends none.
 */
export const KNOWN_ALREADY = 4;

// a miss stays longer: its pill carries the right answer
const FLASH_MS = { correct: 900, miss: 1400 };

/**
 * True when an answer has just taken the combo to a multiple of five, which
 * is when the page throws confetti. An intro card is not an answer, and it
 * comes back with the combo as it was (so does a round sent twice): a combo
 * that merely still stands at five has had its confetti.
 */
export function comboMilestone(before: number, after: number): boolean {
  return after > before && after % 5 === 0;
}

export interface Flash {
  correct: boolean;
  text: string;
  /** goes up with every pill, so the page can replay its animation */
  seq: number;
}

export interface FlowView {
  phase: "loading" | "playing" | "empty" | "done" | "error";
  kind: SessionKind | null;
  definitionLang: string;
  session: SessionStart | null;
  idx: number;
  xp: number;
  combo: number;
  flash: Flash | null;
  summary: SessionSummary | null;
  /** badges earned by an answer, kept for the summary (see mergeBadges) */
  roundBadges: Badge[];
  /** an answer is in and the next round isn't on screen yet */
  answering: boolean;
  /** the answered round stays up until continue() */
  held: boolean;
}

export function initialView(): FlowView {
  return {
    phase: "loading",
    kind: null,
    definitionLang: "en",
    session: null,
    idx: 0,
    xp: 0,
    combo: 0,
    flash: null,
    summary: null,
    roundBadges: [],
    answering: false,
    held: false,
  };
}

export interface FlowDeps {
  api: {
    /** Left out by a page that shows no definitions and has a fixed kind. */
    getSettings?(): Promise<{ definitionLang: string; sessionLength: string }>;
    startSession(kind: SessionKind): Promise<SessionStart>;
    submitRound(sessionId: number, roundId: number, result: RoundResult): Promise<RoundFeedback>;
    finishSession(sessionId: number): Promise<SessionSummary>;
  };
  clock: {
    setTimeout(fn: () => void, ms: number): unknown;
    clearTimeout(id: unknown): void;
    now(): number;
  };
  /** Announces badges on whatever screen is showing; the one call that still happens after leaving. */
  celebrate(badges: Badge[]): void;
  reportError(e: unknown): void;
  /** The pill's text. Left out by a page that shows no pill. */
  flashText?(fb: RoundFeedback, correct: boolean, correctText: string | null): string;
  /** `comboBefore` is the combo as it stood before this answer (see comboMilestone). */
  onFeedback?(fb: RoundFeedback, comboBefore: number): void;
  onDone?(summary: SessionSummary): void;
  revealMs(round: Round): number;
}

export interface FlowOptions {
  /** `?kind=` as it came from the URL */
  requested: string | null;
  /** overrides `requested` and the saved default (the sidecar) */
  fixedKind?: SessionKind;
}

export interface Flow {
  start(): Promise<void>;
  /**
   * A round reports its answer the moment it is given. `quality` is a review
   * card's grade, and on an intro card KNOWN_ALREADY for "Ya me la sé";
   * `hold` keeps the round up until continue().
   */
  answer(correct: boolean, correctText?: string | null, quality?: number, missedWordIds?: number[], hold?: boolean): Promise<void>;
  continue(): void;
  /** While paused (the exit dialog is open) neither the next round nor the summary appears. */
  setPaused(paused: boolean): void;
  /** Finishes the session and announces its badges; the caller then navigates away. */
  exit(): Promise<void>;
  /** The page is going away: go quiet now, finish the session in the background. Safe to call twice. */
  leave(): void;
}

// an api call that throws instead of returning a promise is still just a failed call
function attempt<T>(call: () => Promise<T>): Promise<T> {
  try {
    return call();
  } catch (e) {
    return Promise.reject(e);
  }
}

export function createFlow(view: FlowView, deps: FlowDeps, opts: FlowOptions): Flow {
  const { api, clock } = deps;

  let session: SessionStart | null = null;
  let started = false;
  // Set by leave() and exit(). From then on nothing here writes to the view,
  // starts a timer or calls back into the page: it may already be gone.
  let left = false;
  // finishSession has been sent, or is about to be: it goes exactly once
  let finished = false;
  let paused = false;
  let advancePending = false;
  // The finish came back while paused: what it brought (null when it
  // failed) goes on screen when the pause is over.
  let ending: { summary: SessionSummary | null } | null = null;
  let advanceTimer: unknown = null;
  let flashTimer: unknown = null;
  let flashSeq = 0;
  let roundStart = 0;
  // the submit in flight; it never rejects, so a finish can be chained after it
  let pending: Promise<void> = Promise.resolve();
  // what the pill will say once the answered round makes way for the next
  let answered: { fb: RoundFeedback | null; correct: boolean; correctText: string | null } | null = null;

  // sounds and confetti are extras: one that throws must not strand the session
  function extra(fn: () => void): void {
    try {
      fn();
    } catch (e) {
      deps.reportError(e);
    }
  }

  function celebrate(badges: Badge[]): void {
    if (badges.length > 0) extra(() => deps.celebrate(badges));
  }

  async function start(): Promise<void> {
    if (started || left) return;
    started = true;

    const known: SessionKind | null = opts.fixedKind ?? (isPlayKind(opts.requested) ? opts.requested : null);
    const getSettings = api.getSettings;
    const settingsCall = getSettings ? attempt(() => getSettings()) : Promise.resolve(null);
    // a kind that doesn't hang on the settings starts alongside them
    const earlyCall = known ? attempt(() => api.startSession(known)) : Promise.resolve(null);
    // settled together: neither can fail unobserved, and a session that did
    // start is still known when the other call failed
    const [settings, early] = await Promise.allSettled([settingsCall, earlyCall]);

    let failure: { reason: unknown } | null = null;
    if (settings.status === "rejected") failure = { reason: settings.reason };
    else if (early.status === "rejected") failure = { reason: early.reason };

    let kind = known;
    let s = early.status === "fulfilled" ? early.value : null;
    if (!failure && !left && !kind) {
      const saved = settings.status === "fulfilled" ? settings.value?.sessionLength : null;
      kind = resolveKind(opts.requested, saved);
      const chosen = kind;
      try {
        s = await attempt(() => api.startSession(chosen));
      } catch (e) {
        failure = { reason: e };
      }
    }

    if (left || failure) {
      // nobody will play this session: don't leave it open in the backend
      if (s) {
        const id = s.sessionId;
        void attempt(() => api.finishSession(id)).catch(() => {});
      }
      if (!left && failure) {
        deps.reportError(failure.reason);
        view.phase = "error";
      }
      return;
    }
    if (!s) return;

    session = s;
    view.kind = kind;
    if (settings.status === "fulfilled" && settings.value) view.definitionLang = settings.value.definitionLang;
    view.session = s;
    if (s.rounds.length === 0) {
      view.phase = "empty";
      // Nothing to play, so it is closed now and not when the page goes: an
      // empty session must not sit open in the backend for as long as this
      // screen is up. `finished` first, so that leaving doesn't send it again.
      finished = true;
      const id = s.sessionId;
      void attempt(() => api.finishSession(id)).catch(() => {});
      return;
    }
    roundStart = clock.now();
    view.phase = "playing";
  }

  async function answer(
    correct: boolean,
    correctText: string | null = null,
    quality?: number,
    missedWordIds?: number[],
    hold = false,
  ): Promise<void> {
    if (left || finished || !session || view.phase !== "playing" || view.answering || view.held) return;
    const round = session.rounds[view.idx];
    if (!round) return;
    view.answering = true;
    const t0 = clock.now();
    const sessionId = session.sessionId;
    const result: RoundResult = {
      roundIndex: view.idx,
      correct,
      // the time to answer, without the reveal that follows
      durationMs: Math.max(0, Math.round(t0 - roundStart)),
      quality: quality ?? null,
      missedWordIds: missedWordIds ?? [],
    };

    let fb: RoundFeedback | null = null;
    // Sent at once, whatever happens next: an answer given is always graded,
    // also when the player leaves during the reveal.
    pending = attempt(() => api.submitRound(sessionId, round.id, result)).then(
      (feedback) => {
        fb = feedback;
      },
      // The round is already showing its answer and can't be given again, so
      // the session carries on without this grade.
      (e) => deps.reportError(e),
    );
    await pending;
    const feedback = fb as RoundFeedback | null;

    if (feedback) celebrate(feedback.newBadges);
    if (left) return;
    if (feedback) {
      const comboBefore = view.combo;
      view.xp += feedback.xpGained;
      view.combo = feedback.combo;
      view.roundBadges = mergeBadges(view.roundBadges, feedback.newBadges);
      const onFeedback = deps.onFeedback;
      if (onFeedback) extra(() => onFeedback(feedback, comboBefore));
    }
    answered = { fb: feedback, correct, correctText };

    if (hold) {
      view.held = true;
      return;
    }
    // a slow submit eats into the reveal instead of adding to it
    const wait = deps.revealMs(round) - (clock.now() - t0);
    if (wait > 0) advanceTimer = clock.setTimeout(advance, wait);
    else advance();
  }

  function advance(): void {
    advanceTimer = null;
    if (left || !session) return;
    if (paused) {
      advancePending = true;
      return;
    }
    advancePending = false;
    showFlash();
    if (view.idx + 1 >= session.rounds.length) {
      void finish();
      return;
    }
    view.idx += 1;
    roundStart = clock.now();
    view.answering = false;
  }

  function showFlash(): void {
    const last = answered;
    answered = null;
    const flashText = deps.flashText;
    // no feedback means the submit failed; the error toast says so instead
    if (!last || !last.fb || !flashText) return;
    const fb = last.fb;
    let text = "";
    extra(() => {
      text = flashText(fb, last.correct, last.correctText);
    });
    if (text === "") return;
    // each pill gets its own full time: the one before must not take it down early
    if (flashTimer !== null) clock.clearTimeout(flashTimer);
    const seq = ++flashSeq;
    view.flash = { correct: last.correct, text, seq };
    flashTimer = clock.setTimeout(
      () => {
        flashTimer = null;
        if (view.flash && view.flash.seq === seq) view.flash = null;
      },
      last.correct ? FLASH_MS.correct : FLASH_MS.miss,
    );
  }

  function resume(): void {
    if (left || !view.held) return;
    view.held = false;
    advance();
  }

  function setPaused(value: boolean): void {
    paused = value;
    if (paused) return;
    if (ending) {
      const { summary } = ending;
      ending = null;
      showEnd(summary);
    } else if (advancePending) advance();
  }

  async function finish(): Promise<void> {
    if (finished || !session) return;
    finished = true;
    const id = session.sessionId;
    let summary: SessionSummary | null = null;
    try {
      summary = await attempt(() => api.finishSession(id));
    } catch (e) {
      if (left) return;
      deps.reportError(e);
    }
    if (left) {
      // the summary screen that would have shown them is gone
      if (summary) celebrate(summary.newBadges);
      return;
    }
    // The exit dialog was opened while the finish was on its way: the
    // screen must not change under it.
    if (paused) ending = { summary };
    else showEnd(summary);
  }

  /** The session is over: its summary, or the error screen when the finish failed. */
  function showEnd(summary: SessionSummary | null): void {
    if (!summary) {
      view.phase = "error";
      return;
    }
    view.summary = summary;
    view.phase = "done";
    const onDone = deps.onDone;
    if (onDone) extra(() => onDone(summary));
  }

  function goQuiet(): void {
    left = true;
    if (advanceTimer !== null) clock.clearTimeout(advanceTimer);
    if (flashTimer !== null) clock.clearTimeout(flashTimer);
    advanceTimer = null;
    flashTimer = null;
    // A summary still waiting behind the exit dialog will never be shown:
    // its badges go to the toasts, as after any finish nobody sees.
    const unseen = ending;
    ending = null;
    if (unseen?.summary) celebrate(unseen.summary.newBadges);
  }

  async function exit(): Promise<void> {
    goQuiet();
    if (!session || finished) return;
    finished = true;
    const id = session.sessionId;
    await pending;
    try {
      const summary = await attempt(() => api.finishSession(id));
      celebrate(summary.newBadges);
    } catch (e) {
      deps.reportError(e);
    }
  }

  function leave(): void {
    if (left) return;
    goQuiet();
    if (!session || finished) return;
    finished = true;
    const id = session.sessionId;
    // after the answer in flight, so that it is graded and counted first
    void pending
      .then(() => attempt(() => api.finishSession(id)))
      .then((summary) => celebrate(summary.newBadges))
      .catch(() => {});
  }

  return { start, answer, continue: resume, setPaused, exit, leave };
}
