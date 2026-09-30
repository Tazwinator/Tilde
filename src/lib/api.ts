import { invoke } from "@tauri-apps/api/core";
import type {
  Badge,
  ImportReport,
  MinedSentence,
  PlacementAnswer,
  PlacementItem,
  PlacementResult,
  Profile,
  Round,
  RoundFeedback,
  RoundResult,
  SessionKind,
  SessionStart,
  SessionSummary,
  Settings,
  StatsData,
  TtsInfo,
  WordDetail,
  WordHit,
} from "./contract";

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// ============================================================================
// Mock backend (browser dev mode) — mirrors the wire contract exactly.
// ============================================================================

const LS_KEY = "tilde.mock.v1";

interface MockState {
  xp: number;
  minutesTotal: number;
  sessionsTotal: number;
  bestCombo: number;
  placement: PlacementResult | null;
  settings: Settings;
  badges: Record<string, number | null>;
  knownLemmas: string[];
  learningLemmas: string[];
  cardStrength: Record<string, number>;
  sentences: MinedSentence[];
  nextSentenceId: number;
  weekly: { minutes: number[]; xp: number[]; words: number[] };
  gameMix: Record<string, number>;
}

// ~40 real Spanish words with plausible glosses / frequency ranks / levels.
const DICT: Array<[lemma: string, pos: string, rank: number, en: string, es: string, level: string]> = [
  ["ser", "v", 12, "to be (essence)", "ser", "A1"],
  ["hablar", "v", 45, "to speak", "hablar", "A1"],
  ["comer", "v", 78, "to eat", "comer", "A1"],
  ["vivir", "v", 92, "to live", "vivir", "A1"],
  ["tener", "v", 30, "to have", "tener", "A1"],
  ["estar", "v", 25, "to be (state)", "estar", "A1"],
  ["hacer", "v", 40, "to do, to make", "hacer", "A1"],
  ["poder", "v", 55, "to be able to", "poder", "A1"],
  ["decir", "v", 60, "to say", "decir", "A1"],
  ["ir", "v", 20, "to go", "ir", "A1"],
  ["ver", "v", 70, "to see", "ver", "A1"],
  ["dar", "v", 85, "to give", "dar", "A1"],
  ["saber", "v", 95, "to know", "saber", "A1"],
  ["querer", "v", 110, "to want, to love", "querer", "A2"],
  ["casa", "n", 130, "house", "casa", "A1"],
  ["perro", "n", 340, "dog", "perro", "A1"],
  ["gato", "n", 520, "cat", "gato", "A1"],
  ["agua", "n", 150, "water", "agua", "A1"],
  ["tiempo", "n", 88, "time", "tiempo", "A1"],
  ["mundo", "n", 140, "world", "mundo", "A1"],
  ["día", "n", 65, "day", "día", "A1"],
  ["noche", "n", 200, "night", "noche", "A1"],
  ["hombre", "n", 120, "man", "hombre", "A1"],
  ["mujer", "n", 125, "woman", "mujer", "A1"],
  ["amigo", "n", 210, "friend", "amigo", "A1"],
  ["libro", "n", 280, "book", "libro", "A1"],
  ["ciudad", "n", 360, "city", "ciudad", "A2"],
  ["trabajo", "n", 300, "work, job", "trabajo", "A2"],
  ["niño", "n", 190, "child", "niño", "A1"],
  ["comida", "n", 310, "food", "comida", "A1"],
  ["grande", "adj", 105, "big", "grande", "A1"],
  ["pequeño", "adj", 260, "small", "pequeño", "A1"],
  ["bueno", "adj", 98, "good", "bueno", "A1"],
  ["feliz", "adj", 420, "happy", "feliz", "A2"],
  ["rápido", "adj", 460, "fast", "rápido", "A2"],
  ["siempre", "adv", 175, "always", "siempre", "A1"],
  ["ahora", "adv", 115, "now", "ahora", "A1"],
  ["gracias", "interj", 230, "thank you", "gracias", "A1"],
  ["hola", "interj", 350, "hello", "hola", "A1"],
  ["adiós", "interj", 580, "goodbye", "adiós", "A1"],
  ["canción", "n", 890, "song", "canción", "B1"],
  ["mañana", "n", 165, "morning, tomorrow", "mañana", "A1"],
];

const MIN_RANK_KNOWN = 220; // mock profile: words below this rank are known (247 total)

interface WordEntry {
  wordId: number;
  lemma: string;
  pos: string | null;
  rank: number;
  glossEn: string | null;
  glossEs: string | null;
  level: string;
}

const WORDS: WordEntry[] = DICT.map(([lemma, pos, rank, en, es, level], i) => ({
  wordId: i + 1,
  lemma,
  pos,
  rank,
  glossEn: en,
  glossEs: es,
  level,
}));

const byLemma = new Map(WORDS.map((w) => [w.lemma, w]));

function defaultState(): MockState {
  return {
    xp: 4820,
    minutesTotal: 512,
    sessionsTotal: 68,
    bestCombo: 11,
    placement: null,
    settings: { definitionLang: "en", sessionLength: "standard", soundEnabled: true, targetLang: "es-ES" },
    badges: { first_session: Date.now() - 86400000 * 40, combo_5: Date.now() - 86400000 * 21, words_50: Date.now() - 86400000 * 12, deep_dive: null, night_owl: null, explorer: null },
    knownLemmas: WORDS.filter((w) => w.rank < MIN_RANK_KNOWN).map((w) => w.lemma),
    learningLemmas: ["canción", "feliz", "ciudad"],
    cardStrength: {},
    sentences: [
      { id: 1, es: "Ayer vi una película española y entendí casi todo.", en: "Yesterday I watched a Spanish movie and understood almost everything.", source: "película: El hoyo", createdAt: Date.now() - 86400000 * 3, inDeck: true },
      { id: 2, es: "¿Cuánto tiempo llevas estudiando español?", en: "How long have you been studying Spanish?", source: "podcast: Nómadas", createdAt: Date.now() - 86400000 * 2, inDeck: false },
      { id: 3, es: "Me gusta caminar por la ciudad por la noche.", en: "I like walking around the city at night.", source: "libro: La sombra del viento", createdAt: Date.now() - 86400000, inDeck: false },
    ],
    nextSentenceId: 4,
    weekly: {
      minutes: [35, 52, 28, 61, 44, 72, 58, 66],
      xp: [310, 455, 260, 590, 410, 660, 520, 610],
      words: [180, 192, 201, 214, 221, 233, 240, 247],
    },
    gameMix: { choice: 38, listen: 24, match: 14, build: 10, cloze: 8, review: 6 },
  };
}

let state: MockState = typeof localStorage !== "undefined" ? load() : defaultState();

function load(): MockState {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) return { ...defaultState(), ...(JSON.parse(raw) as MockState) };
  } catch {
    /* ignore */
  }
  return defaultState();
}

function save(): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(state));
  } catch {
    /* ignore */
  }
}

const LEVEL_NAMES = ["Principiante", "Curioso", "Aprendiz", "Conversador", "Charlatán", "Bilingüe en ciernes", "Maestro del tilde"];
const XP_PER_LEVEL = 1200;

function levelInfo(xp: number): { level: number; name: string; xpIntoLevel: number; xpForNext: number } {
  const level = Math.floor(xp / XP_PER_LEVEL) + 1;
  return {
    level,
    name: LEVEL_NAMES[Math.min(level - 1, LEVEL_NAMES.length - 1)],
    xpIntoLevel: xp % XP_PER_LEVEL,
    xpForNext: XP_PER_LEVEL,
  };
}

const ALL_BADGES: Badge[] = [
  { id: "first_session", name: "Primer paso", description: "Completa tu primera sesión", earnedAt: null },
  { id: "combo_5", name: "Racha de fuego", description: "Consigue un combo de 5", earnedAt: null },
  { id: "words_50", name: "Cincuentenario", description: "Aprende 50 palabras nuevas", earnedAt: null },
  { id: "deep_dive", name: "Buceo profundo", description: "Termina una sesión profunda", earnedAt: null },
  { id: "night_owl", name: "Búho nocturno", description: "Juega una sesión después de las 22h", earnedAt: null },
  { id: "explorer", name: "Explorador", description: "Mina 10 frases de contenido real", earnedAt: null },
];

function profileMock(): Profile {
  const li = levelInfo(state.xp);
  return {
    xp: state.xp,
    level: li.level,
    levelName: li.name,
    levelProgress: li.xpIntoLevel / li.xpForNext,
    wordsKnown: 247,
    wordsLearning: state.learningLemmas.length + 5,
    minutesTotal: state.minutesTotal,
    cefrEstimate: state.placement?.cefrEstimate ?? "A2",
    reviewsDue: 14,
    newWordsReady: 9,
    daysSinceLastSession: 4,
    sessionsTotal: state.sessionsTotal,
    badgesCount: ALL_BADGES.filter((b) => state.badges[b.id] != null).length,
    definitionLang: state.settings.definitionLang,
    ttsAvailable: typeof window !== "undefined" && "speechSynthesis" in window,
    targetLang: "es-ES",
  };
}

function wordHit(w: WordEntry): WordHit {
  return { ...w, known: state.knownLemmas.includes(w.lemma) || state.learningLemmas.includes(w.lemma) };
}

function shuffle<T>(arr: T[]): T[] {
  const a = [...arr];
  for (let i = a.length - 1; i > 0; i--) {
    const j = (Math.random() * (i + 1)) | 0;
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

function pick<T>(arr: T[], n: number): T[] {
  return shuffle(arr).slice(0, n);
}

function distractors(w: WordEntry, n: number): WordEntry[] {
  const pool = WORDS.filter((x) => x.wordId !== w.wordId);
  return pick(pool, n);
}

// --- example sentences keyed by lemma -------------------------------------
const SENTENCES: Array<{ lemma: string; es: string; en: string }> = [
  { lemma: "hablar", es: "Quiero hablar español todos los días.", en: "I want to speak Spanish every day." },
  { lemma: "comer", es: "Vamos a comer paella este domingo.", en: "We are going to eat paella this Sunday." },
  { lemma: "vivir", es: "Mi abuela vive en un pueblo pequeño.", en: "My grandmother lives in a small village." },
  { lemma: "ser", es: "Ser valiente no significa no tener miedo.", en: "Being brave doesn't mean having no fear." },
  { lemma: "estar", es: "El café está caliente, ten cuidado.", en: "The coffee is hot, be careful." },
  { lemma: "tener", es: "No tengo tiempo ahora mismo.", en: "I don't have time right now." },
  { lemma: "casa", es: "Mi casa está cerca de la playa.", en: "My house is near the beach." },
  { lemma: "perro", es: "El perro de mi vecino ladra mucho.", en: "My neighbor's dog barks a lot." },
  { lemma: "agua", es: "¿Me traes un vaso de agua, por favor?", en: "Could you bring me a glass of water, please?" },
  { lemma: "tiempo", es: "Con el tiempo, todo se aprende.", en: "With time, everything is learned." },
  { lemma: "mundo", es: "Hay un mundo entero por descubrir.", en: "There's a whole world to discover." },
  { lemma: "día", es: "Cada día aprendo algo nuevo.", en: "Every day I learn something new." },
  { lemma: "mundo", es: "El mundo es un pañuelo.", en: "It's a small world." },
  { lemma: "noche", es: "Esta noche vamos a bailar salsa.", en: "Tonight we're going to dance salsa." },
  { lemma: "libro", es: "Leo un libro en español cada mes.", en: "I read a book in Spanish every month." },
  { lemma: "ciudad", es: "Barcelona es una ciudad fascinante.", en: "Barcelona is a fascinating city." },
  { lemma: "amigo", es: "Un buen amigo escucha sin juzgar.", en: "A good friend listens without judging." },
  { lemma: "gracias", es: "Muchas gracias por tu ayuda.", en: "Thank you very much for your help." },
  { lemma: "siempre", es: "Siempre desayuno pan tostado.", en: "I always have toast for breakfast." },
  { lemma: "trabajo", es: "Mi trabajo empieza a las nueve.", en: "My work starts at nine." },
];

function sentenceFor(w: WordEntry): { es: string; en: string } | null {
  return SENTENCES.find((s) => s.lemma === w.lemma) ?? null;
}

// --- conjugations -----------------------------------------------------------
const PERSONS = ["yo", "tú", "él/ella", "nosotros", "vosotros", "ellos/ellas"];
const ENDINGS: Record<string, string[]> = {
  ar: ["o", "as", "a", "amos", "áis", "an"],
  er: ["o", "es", "e", "emos", "éis", "en"],
  ir: ["o", "es", "e", "imos", "ís", "en"],
};
const IRREG: Record<string, Record<string, string[]>> = {
  ser: { presente: ["soy", "eres", "es", "somos", "sois", "son"] },
  estar: { presente: ["estoy", "estás", "está", "estamos", "estáis", "están"] },
  tener: { presente: ["tengo", "tienes", "tiene", "tenemos", "tenéis", "tienen"] },
  hacer: { presente: ["hago", "haces", "hace", "hacemos", "hacéis", "hacen"] },
  decir: { presente: ["digo", "dices", "dice", "decimos", "decís", "dicen"] },
  ir: { presente: ["voy", "vas", "va", "vamos", "vais", "van"] },
  saber: { presente: ["sé", "sabes", "sabe", "sabemos", "sabéis", "saben"] },
  querer: { presente: ["quiero", "quieres", "quiere", "queremos", "queréis", "quieren"] },
  poder: { presente: ["puedo", "puedes", "puede", "podemos", "podéis", "pueden"] },
  ver: { presente: ["veo", "ves", "ve", "vemos", "veis", "ven"] },
  dar: { presente: ["doy", "das", "da", "damos", "dais", "dan"] },
};

function conjugationsFor(w: WordEntry): Array<{ tense: string; person: string; form: string }> | null {
  if (w.pos !== "v") return null;
  const rows: Array<{ tense: string; person: string; form: string }> = [];
  const pres = IRREG[w.lemma]?.presente;
  if (pres) {
    PERSONS.forEach((p, i) => rows.push({ tense: "presente", person: p, form: pres[i] }));
  } else {
    const stem = w.lemma.slice(0, -2);
    const kind = w.lemma.endsWith("ar") ? "ar" : w.lemma.endsWith("er") ? "er" : "ir";
    ENDINGS[kind].forEach((e, i) => rows.push({ tense: "presente", person: PERSONS[i], form: stem + e }));
  }
  if (w.lemma === "ser") {
    ["era", "eras", "era", "éramos", "erais", "eran"].forEach((f, i) =>
      rows.push({ tense: "imperfecto", person: PERSONS[i], form: f }),
    );
  } else if (w.lemma === "hablar") {
    const stem = "habl";
    ["aba", "abas", "aba", "ábamos", "abais", "aban"].forEach((e, i) =>
      rows.push({ tense: "imperfecto", person: PERSONS[i], form: stem + e }),
    );
  }
  return rows;
}

// --- session mock -------------------------------------------------------------
interface MockSession {
  kind: SessionKind;
  rounds: Round[];
  results: RoundResult[];
  xp: number;
  combo: number;
  bestCombo: number;
  newWords: string[];
  nextRoundId: number;
}

const sessions = new Map<number, MockSession>();
let nextSessionId = 1;

function sessionRoundCount(kind: SessionKind): number {
  return { quick: 8, standard: 12, deep: 20, review_only: 8, sidecar: 10 }[kind];
}

function newWordRound(id: number, w: WordEntry): Round {
  const s = sentenceFor(w);
  return { type: "new_word", id, word: { ...w }, exampleEs: s?.es ?? null, exampleEn: s?.en ?? null, audioBase64: null };
}

function choiceRound(id: number, w: WordEntry): Round {
  const promptLang: "es" | "en" = Math.random() < 0.5 ? "es" : "en";
  const opts = shuffle([w, ...distractors(w, 3)]);
  return {
    type: "choice",
    id,
    wordId: w.wordId,
    prompt: promptLang === "es" ? w.lemma : (w.glossEn ?? w.lemma),
    promptLang,
    options: opts.map((o) => (promptLang === "es" ? (o.glossEn ?? o.lemma) : o.lemma)),
    answerIndex: opts.findIndex((o) => o.wordId === w.wordId),
    audioBase64: null,
  };
}

function listenRound(id: number, w: WordEntry): Round {
  const opts = shuffle([w, ...distractors(w, 3)]);
  return {
    type: "listen",
    id,
    wordId: w.wordId,
    audioBase64: null,
    options: opts.map((o) => o.lemma),
    answerIndex: opts.findIndex((o) => o.wordId === w.wordId),
  };
}

function listenTypeRound(id: number, w: WordEntry): Round {
  const s = sentenceFor(w) ?? { es: `Hoy quiero ${w.lemma} algo nuevo.`, en: "Today I want to do something new." };
  return { type: "listen_type", id, wordId: w.wordId, sentenceEs: s.es, audioBase64: null, answer: s.es };
}

function matchRound(id: number): Round {
  const chosen = pick(WORDS, 4);
  return {
    type: "match",
    id,
    pairs: chosen.map((w) => ({ wordId: w.wordId, es: w.lemma, en: w.glossEn ?? w.lemma })),
  };
}

function buildRound(id: number, w: WordEntry): Round {
  const s = sentenceFor(w) ?? { es: `Me gusta ${w.lemma} con mis amigos.`, en: "I like to do it with my friends." };
  const tiles = shuffle(s.es.replace(/[.,!?¿¡]/g, "").split(/\s+/));
  return { type: "build", id, wordId: w.wordId, sentenceEs: s.es, sentenceEn: s.en, tiles, answer: s.es.replace(/[.,!?¿¡]/g, "") };
}

function clozeRound(id: number, w: WordEntry): Round {
  const s = sentenceFor(w) ?? { es: `Cada día ${w.lemma} un poco más.`, en: "Every day a little more." };
  const masked = s.es.replace(w.lemma, "___");
  const opts = shuffle([w.lemma, ...distractors(w, 3).map((d) => d.lemma)]);
  return {
    type: "cloze",
    id,
    wordId: w.wordId,
    sentenceEs: masked,
    sentenceEn: s.en,
    options: opts,
    answerIndex: opts.indexOf(w.lemma),
  };
}

function conjugationRound(id: number, w: WordEntry): Round {
  const conj = conjugationsFor(w);
  const row = conj?.find((r) => r.tense === "presente" && (r.person === "tú" || r.person === "yo")) ?? {
    person: "tú",
    form: w.lemma,
  };
  const answer = row.form;
  return {
    type: "conjugation",
    id,
    wordId: w.wordId,
    verb: w.lemma,
    tense: "presente",
    person: row.person,
    answer,
    hint: answer[0] + "·".repeat(Math.max(answer.length - 1, 1)),
  };
}

function reviewCardRound(id: number, w: WordEntry): Round {
  const s = sentenceFor(w);
  return { type: "review_card", id, wordId: w.wordId, es: w.lemma, en: w.glossEn ?? w.lemma, exampleEs: s?.es ?? null, exampleEn: s?.en ?? null, audioBase64: null };
}

function buildRounds(kind: SessionKind): Round[] {
  const n = sessionRoundCount(kind);
  const rounds: Round[] = [];
  let id = 1;
  const learning = state.learningLemmas.map((l) => byLemma.get(l)).filter((w): w is WordEntry => !!w);
  const known = state.knownLemmas.map((l) => byLemma.get(l)).filter((w): w is WordEntry => !!w);
  const fresh = WORDS.filter((w) => !state.knownLemmas.includes(w.lemma) && !state.learningLemmas.includes(w.lemma));

  if (kind === "sidecar") {
    for (let i = 0; i < n; i++) {
      const w = pick(known.length ? known : WORDS, 1)[0];
      const roll = Math.random();
      rounds.push(roll < 0.4 ? listenRound(id++, w) : roll < 0.75 ? choiceRound(id++, w) : listenTypeRound(id++, w));
    }
    return rounds;
  }

  if (kind === "review_only") {
    for (let i = 0; i < n; i++) {
      const w = pick(learning.length ? learning : WORDS, 1)[0];
      const roll = Math.random();
      rounds.push(roll < 0.45 ? reviewCardRound(id++, w) : roll < 0.75 ? choiceRound(id++, w) : listenRound(id++, w));
    }
    return rounds;
  }

  // learning sessions: start with 1-2 new words, then a mixed game flow
  const newCount = kind === "quick" ? 1 : kind === "standard" ? 2 : 4;
  const newPool = pick(fresh.length >= newCount ? fresh : WORDS, newCount);
  newPool.forEach((w) => rounds.push(newWordRound(id++, w)));

  const games = [choiceRound, listenRound, matchRound, clozeRound, buildRound, listenTypeRound, conjugationRound];
  while (rounds.length < n) {
    const w = pick([...learning, ...known, ...newPool], 1)[0] ?? WORDS[0];
    if (rounds.length === newCount) rounds.push(matchRound(id++));
    else {
      const g = games[(Math.random() * games.length) | 0];
      rounds.push(g(id++, w));
    }
  }
  return rounds;
}

function mockSubmitRound(sessionId: number, roundId: number, result: RoundResult): RoundFeedback {
  const s = sessions.get(sessionId);
  if (!s) throw new Error("session not found");
  s.results.push(result);
  const round = s.rounds.find((r) => r.id === roundId);
  const correct = result.correct;
  let xpGained = 0;
  let combo = 0;
  let levelUp: import("./contract").LevelInfo | null = null;
  const newBadges: Badge[] = [];
  if (correct) {
    s.combo += 1;
    s.bestCombo = Math.max(s.bestCombo, s.combo);
    xpGained = 10 + Math.min(s.combo - 1, 5) * 2;
    combo = s.combo;
  } else {
    s.combo = 0;
    combo = 0;
  }
  if (round?.type === "new_word" && correct) {
    const lemma = round.word.lemma;
    s.newWords.push(lemma);
    if (!state.learningLemmas.includes(lemma) && !state.knownLemmas.includes(lemma)) {
      state.learningLemmas.push(lemma);
    }
  }
  const prevXp = state.xp;
  state.xp += xpGained;
  if (Math.floor(state.xp / XP_PER_LEVEL) > Math.floor(prevXp / XP_PER_LEVEL)) {
    levelUp = levelInfo(state.xp);
  }
  if (s.bestCombo >= 5 && state.badges["combo_5"] == null) {
    state.badges["combo_5"] = Date.now();
    newBadges.push(ALL_BADGES[1]);
  }
  save();
  const correctAnswer =
    round == null
      ? null
      : round.type === "choice" || round.type === "listen" || round.type === "cloze"
        ? round.options[round.answerIndex]
        : round.type === "conjugation"
          ? round.answer
          : round.type === "listen_type" || round.type === "build"
            ? round.answer
            : round.type === "review_card"
              ? round.en
              : null;
  return { correctAnswer, xpGained, combo, levelUp, newBadges };
}

function mockFinishSession(sessionId: number): SessionSummary {
  const s = sessions.get(sessionId);
  if (!s) throw new Error("session not found");
  const minutes = { quick: 3, standard: 6, deep: 12, sidecar: 5, review_only: 4 }[s.kind];
  const correct = s.results.filter((r) => r.correct).length;
  const accuracy = s.results.length ? correct / s.results.length : 0;
  state.minutesTotal += minutes;
  state.sessionsTotal += 1;
  state.weekly.minutes[state.weekly.minutes.length - 1] += minutes;
  state.weekly.xp[state.weekly.xp.length - 1] += s.xp;
  state.weekly.words[state.weekly.words.length - 1] += s.newWords.length;
  state.weekly.words = state.weekly.words.map((w, i) => Math.max(w, state.weekly.words[i - 1] ?? 0));
  const kindTag: Record<SessionKind, string> = { quick: "choice", standard: "choice", deep: "listen", sidecar: "listen", review_only: "review" };
  state.gameMix[kindTag[s.kind]] = (state.gameMix[kindTag[s.kind]] ?? 0) + minutes;
  const newBadges: Badge[] = [];
  if (s.kind === "deep" && state.badges["deep_dive"] == null) {
    state.badges["deep_dive"] = Date.now();
    newBadges.push(ALL_BADGES[3]);
  }
  const summary: SessionSummary = {
    xp: s.xp,
    rounds: s.results.length,
    correct,
    minutes,
    newWords: s.newWords,
    accuracy,
    levelUp: null,
    newBadges,
    bestCombo: s.bestCombo,
  };
  summary.levelUp = Math.floor(state.xp / XP_PER_LEVEL) > Math.floor((state.xp - s.xp) / XP_PER_LEVEL) ? levelInfo(state.xp) : null;
  summary.newBadges = newBadges;
  sessions.delete(sessionId);
  save();
  return summary;
}

function norm(s: string): string {
  return s
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[.,!?¿¡;:]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

// ============================================================================
// API — single object, every method async. Tauri invoke when available,
// realistic in-memory mock otherwise.
// ============================================================================

export const api = {
  isTauri,

  async profile(): Promise<Profile> {
    if (isTauri) return invoke<Profile>("profile_get");
    return profileMock();
  },

  async startSession(kind: SessionKind): Promise<SessionStart> {
    if (isTauri) return invoke<SessionStart>("session_start", { kind });
    const sessionId = nextSessionId++;
    const rounds = buildRounds(kind);
    const session: MockSession = { kind, rounds, results: [], xp: 0, combo: 0, bestCombo: 0, newWords: [], nextRoundId: rounds.length };
    sessions.set(sessionId, session);
    return { sessionId, kind, rounds };
  },

  async submitRound(sessionId: number, roundId: number, result: RoundResult): Promise<RoundFeedback> {
    if (isTauri) return invoke<RoundFeedback>("round_submit", { sessionId, roundId, result });
    return mockSubmitRound(sessionId, roundId, result);
  },

  async finishSession(sessionId: number): Promise<SessionSummary> {
    if (isTauri) return invoke<SessionSummary>("session_finish", { sessionId });
    return mockFinishSession(sessionId);
  },

  async placementStatus(): Promise<PlacementResult | null> {
    if (isTauri) return invoke<PlacementResult | null>("placement_status");
    return state.placement;
  },

  async placementStart(): Promise<PlacementItem[]> {
    if (isTauri) return invoke<PlacementItem[]>("placement_start");
    const bands: Array<[number, string]> = [
      [0, "A1"],
      [30, "A1"],
      [60, "A2"],
      [110, "A2"],
      [180, "B1"],
      [280, "B1"],
      [420, "B2"],
      [580, "B2"],
      [700, "C1"],
      [800, "C1"],
    ];
    const chosen: PlacementItem[] = [];
    for (const [rank, band] of bands) {
      const w = WORDS.filter((x) => x.rank >= rank)[0];
      if (!w || chosen.some((c) => c.wordId === w.wordId)) continue;
      const opts = shuffle([w, ...distractors(w, 3)]);
      chosen.push({
        wordId: w.wordId,
        lemma: w.lemma,
        options: opts.map((o) => o.glossEn ?? o.lemma),
        answerIndex: opts.findIndex((o) => o.wordId === w.wordId),
        band,
      });
    }
    return shuffle(chosen).slice(0, 12);
  },

  async placementSubmit(answers: PlacementAnswer[], _startBand?: string): Promise<PlacementResult> {
    if (isTauri) return invoke<PlacementResult>("placement_submit", { answers, startBand: _startBand ?? null });
    const correct = answers.filter((a) => a.correct).length;
    const cefr = correct >= 10 ? "B2" : correct >= 8 ? "B1" : correct >= 5 ? "A2" : "A1";
    const frontier = 60 + correct * 45;
    state.placement = { frontierRank: frontier, wordsMarkedKnown: correct * 6, cefrEstimate: cefr };
    save();
    return state.placement;
  },

  async searchWords(query: string, limit = 50): Promise<WordHit[]> {
    if (isTauri) return invoke<WordHit[]>("word_search", { query, limit });
    const q = norm(query);
    const hits = WORDS.filter((w) => !q || norm(w.lemma).includes(q) || norm(w.glossEn ?? "").includes(q));
    return hits.slice(0, limit).map(wordHit);
  },

  async wordDetail(wordId: number): Promise<WordDetail> {
    if (isTauri) return invoke<WordDetail>("word_detail", { wordId });
    const w = WORDS.find((x) => x.wordId === wordId);
    if (!w) throw new Error("word not found");
    const known = state.knownLemmas.includes(w.lemma);
    const learning = state.learningLemmas.includes(w.lemma);
    const strength = state.cardStrength[w.lemma] ?? (learning ? 0.4 : known ? 0.95 : 0);
    const reps = learning ? 3 : known ? 8 : 0;
    const card = learning || known
      ? { state: learning ? "learning" : "review", dueInDays: learning ? 1 : 12, reps, lapses: 0, strength }
      : null;
    const s = sentenceFor(w);
    return {
      word: wordHit(w),
      audioAvailable: true,
      card,
      conjugations: conjugationsFor(w),
      sentences: s ? [{ id: wordId * 100, es: s.es, en: s.en, mined: state.sentences.some((x) => norm(x.es) === norm(s.es)) }] : [],
    };
  },

  async markWordKnown(wordId: number): Promise<void> {
    if (isTauri) return invoke<void>("word_mark_known", { wordId });
    const w = WORDS.find((x) => x.wordId === wordId);
    if (w && !state.knownLemmas.includes(w.lemma)) {
      state.knownLemmas.push(w.lemma);
      state.learningLemmas = state.learningLemmas.filter((l) => l !== w.lemma);
      state.cardStrength[w.lemma] = 1;
    }
    save();
  },

  async resetWord(wordId: number): Promise<void> {
    if (isTauri) return invoke<void>("word_reset", { wordId });
    const w = WORDS.find((x) => x.wordId === wordId);
    if (w) {
      state.knownLemmas = state.knownLemmas.filter((l) => l !== w.lemma);
      state.learningLemmas = state.learningLemmas.filter((l) => l !== w.lemma);
      state.cardStrength[w.lemma] = 0;
    }
    save();
  },

  async speak(text: string): Promise<void> {
    if (isTauri) {
      const b64 = await invoke<string | null>("tts_speak", { text });
      if (b64) {
        try {
          void new Audio(`data:audio/wav;base64,${b64}`).play();
        } catch {
          /* audio device unavailable */
        }
      }
      return;
    }
    if (typeof speechSynthesis !== "undefined") {
      const u = new SpeechSynthesisUtterance(text);
      u.lang = "es-ES";
      speechSynthesis.speak(u);
    }
  },

  async ttsInfo(): Promise<TtsInfo> {
    if (isTauri) return invoke<TtsInfo>("tts_info");
    const ok = typeof window !== "undefined" && "speechSynthesis" in window;
    return { engine: ok ? "speechSynthesis (navegador)" : "no disponible", voice: ok ? "es-ES" : "—" };
  },

  async importSrt(title: string, text: string): Promise<ImportReport> {
    if (isTauri) return invoke<ImportReport>("srt_import", { title, text });
    const lines = text.split(/\r?\n/).map((l) => l.trim()).filter((l) => l && !/^\d+$/.test(l) && !l.includes("-->"));
    const sentencesFound = Math.max(lines.length, 1);
    const matchedWords: WordEntry[] = [];
    let wordsMatched = 0;
    for (const w of WORDS) {
      const re = new RegExp(`\\b${w.lemma.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "i");
      const count = lines.filter((l) => re.test(norm(l))).length;
      if (count > 0) {
        wordsMatched += count;
        matchedWords.push(w);
      }
    }
    const added = Math.min(sentencesFound, 12);
    for (let i = 0; i < added; i++) {
      state.sentences.unshift({
        id: state.nextSentenceId++,
        es: lines[i % lines.length] ?? title,
        en: null,
        source: title,
        createdAt: Date.now(),
        inDeck: false,
      });
    }
    save();
    return {
      fileTitle: title,
      sentencesFound,
      sentencesAdded: added,
      wordsMatched,
      topWords: matchedWords.sort((a, b) => a.rank - b.rank).slice(0, 8).map(wordHit),
    };
  },

  async listSentences(): Promise<MinedSentence[]> {
    if (isTauri) return invoke<MinedSentence[]>("sentences_list");
    return [...state.sentences];
  },

  async addSentenceToDeck(id: number): Promise<void> {
    if (isTauri) return invoke<void>("sentence_add_to_deck", { id });
    const s = state.sentences.find((x) => x.id === id);
    if (s) s.inDeck = true;
    save();
  },

  async stats(): Promise<StatsData> {
    if (isTauri) return invoke<StatsData>("stats_get");
    const weekLabels = ["S-7", "S-6", "S-5", "S-4", "S-3", "S-2", "S-1", "Esta"];
    return {
      minutesPerWeek: weekLabels.map((label, i) => ({ label, value: state.weekly.minutes[i] ?? 0 })),
      xpPerWeek: weekLabels.map((label, i) => ({ label, value: state.weekly.xp[i] ?? 0 })),
      cumulativeWords: weekLabels.map((label, i) => ({ label, value: state.weekly.words[i] ?? 0 })),
      accuracyAllTime: 0.86,
      gameMix: Object.entries(state.gameMix).map(([label, value]) => ({ label, value })),
    };
  },

  async getSettings(): Promise<Settings> {
    if (isTauri) return invoke<Settings>("settings_get");
    return { ...state.settings };
  },

  async setSettings(settings: Settings): Promise<Settings> {
    if (isTauri) return invoke<Settings>("settings_set", { settings });
    state.settings = { ...settings };
    save();
    return { ...state.settings };
  },

  async listBadges(): Promise<Badge[]> {
    if (isTauri) return invoke<Badge[]>("badges_list");
    return ALL_BADGES.map((b) => ({ ...b, earnedAt: state.badges[b.id] ?? null }));
  },

  async exportBackup(): Promise<string> {
    if (isTauri) return invoke<string>("backup_export");
    return "/home/usuario/.local/share/tilde/backups/tilde-backup-2026-09-23.json";
  },

  async importBackup(bytes: Uint8Array): Promise<void> {
    if (isTauri) return invoke<void>("backup_import", { bytes: Array.from(bytes) });
    // mock: pretend restore succeeded, bump a couple of counters
    state.sessionsTotal += 1;
    save();
  },

  async resetProgress(): Promise<void> {
    if (isTauri) return invoke<void>("progress_reset");
    state = defaultState();
    state.placement = null;
    save();
  },
};

export { norm };
