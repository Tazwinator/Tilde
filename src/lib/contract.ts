// Wire contract mirroring crates/tilde-core/src/types.rs (serde camelCase).

export type SessionKind = "quick" | "standard" | "deep" | "sidecar" | "review_only";

export interface LevelInfo {
  level: number;
  name: string;
  xpIntoLevel: number;
  xpForNext: number;
}

export interface Badge {
  id: string;
  name: string;
  description: string;
  earnedAt: number | null;
}

export interface Profile {
  xp: number;
  level: number;
  levelName: string;
  levelProgress: number;
  wordsKnown: number;
  wordsLearning: number;
  minutesTotal: number;
  cefrEstimate: string;
  reviewsDue: number;
  newWordsReady: number;
  daysSinceLastSession: number;
  sessionsTotal: number;
  badgesCount: number;
  definitionLang: string;
  ttsAvailable: boolean;
  targetLang: string;
}

export interface WordCard {
  wordId: number;
  lemma: string;
  pos: string | null;
  rank: number;
  glossEn: string | null;
  glossEs: string | null;
  level: string;
}

export interface MatchPair {
  wordId: number;
  es: string;
  en: string;
}

export type Round =
  | ({ type: "new_word"; id: number; word: WordCard; exampleEs: string | null; exampleEn: string | null; audioBase64: string | null })
  | ({ type: "choice"; id: number; wordId: number; prompt: string; promptLang: "es" | "en"; options: string[]; answerIndex: number; audioBase64: string | null })
  | ({ type: "match"; id: number; pairs: MatchPair[] })
  | ({ type: "listen"; id: number; wordId: number; es: string; audioBase64: string | null; options: string[]; answerIndex: number })
  | ({ type: "listen_type"; id: number; wordId: number; sentenceEs: string; audioBase64: string | null; answer: string })
  | ({ type: "build"; id: number; wordId: number; sentenceEs: string; sentenceEn: string; tiles: string[]; answer: string })
  | ({ type: "cloze"; id: number; wordId: number; sentenceEs: string; sentenceEn: string; options: string[]; answerIndex: number })
  | ({ type: "conjugation"; id: number; wordId: number; verb: string; tense: string; person: string; answer: string; hint: string })
  | ({ type: "review_card"; id: number; wordId: number; es: string; en: string; exampleEs: string | null; exampleEn: string | null; audioBase64: string | null });

export interface RoundResult {
  roundIndex: number;
  correct: boolean;
  durationMs: number;
  quality?: number | null; // FSRS grade 1..4 for review_card rounds
  missedWordIds?: number[]; // match rounds: pairs the player got wrong at least once
}

export interface RoundFeedback {
  correctAnswer: string | null;
  xpGained: number;
  combo: number;
  levelUp: LevelInfo | null;
  newBadges: Badge[];
}

export interface SessionStart {
  sessionId: number;
  kind: SessionKind;
  rounds: Round[];
}

export interface SessionSummary {
  xp: number;
  rounds: number;
  correct: number;
  minutes: number;
  newWords: string[];
  accuracy: number;
  levelUp: LevelInfo | null;
  newBadges: Badge[];
  bestCombo: number;
}

export interface PlacementItem {
  wordId: number;
  lemma: string;
  options: string[];
  answerIndex: number;
  band: string;
}

export interface PlacementAnswer {
  wordId: number;
  correct: boolean;
}

export interface PlacementResult {
  frontierRank: number;
  wordsMarkedKnown: number;
  cefrEstimate: string;
}

export interface WordHit {
  wordId: number;
  lemma: string;
  pos: string | null;
  rank: number;
  glossEn: string | null;
  glossEs: string | null;
  level: string;
  known: boolean;
}

export interface ConjRow {
  tense: string;
  person: string;
  form: string;
}

export interface ExSentence {
  id: number;
  es: string;
  en: string | null;
  mined: boolean;
}

export interface CardStateInfo {
  state: string;
  dueInDays: number;
  reps: number;
  lapses: number;
  strength: number;
}

export interface WordDetail {
  word: WordHit;
  audioAvailable: boolean;
  card: CardStateInfo | null;
  conjugations: ConjRow[] | null;
  sentences: ExSentence[];
}

export interface ImportReport {
  fileTitle: string;
  sentencesFound: number;
  sentencesAdded: number;
  wordsMatched: number;
  topWords: WordHit[];
}

export interface MinedSentence {
  id: number;
  es: string;
  en: string | null;
  source: string;
  createdAt: number;
  inDeck: boolean;
}

export interface SeriesPoint {
  label: string;
  value: number;
}

export interface StatsData {
  minutesPerWeek: SeriesPoint[];
  xpPerWeek: SeriesPoint[];
  cumulativeWords: SeriesPoint[];
  accuracyAllTime: number;
  gameMix: SeriesPoint[];
}

export interface Settings {
  definitionLang: string; // "en" | "es"
  sessionLength: string; // "quick" | "standard" | "deep"
  soundEnabled: boolean;
  targetLang: string;
}

export interface TtsInfo {
  engine: string;
  voice: string;
}

export type BackupKind = "manual" | "auto" | "before-reset" | "before-restore";

export interface BackupInfo {
  path: string;
  name: string;
  kind: BackupKind;
  createdAt: number; // unix seconds
  sizeBytes: number;
}

export interface BackupFolder {
  dir: string;
  isDefault: boolean;
  backups: BackupInfo[]; // newest first
}
