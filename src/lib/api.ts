import { invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import type {
  BackupFolder,
  Badge,
  ImportReport,
  PlacementAnswer,
  PlacementItem,
  PlacementResult,
  Profile,
  Round,
  RoundFeedback,
  RoundResult,
  SentencePage,
  SessionKind,
  SessionStart,
  SessionSummary,
  Settings,
  StatsData,
  TtsInfo,
  WordDetail,
  WordHit,
} from "./contract";

// Thin typed wrapper over the Tauri commands in src-tauri/src/lib.rs.
// Run the UI with `npm run tauri dev`; there is no browser-only backend.

// The slowest command is speech: two engines with a 10 s limit each.
const TIMEOUT_MS = 30_000;

/** `invoke`, but a call that never answers (a command that crashed) fails
 * after a while instead of leaving a spinner up forever. */
async function invoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`Tilde no ha respondido (${cmd}).`)), TIMEOUT_MS);
  });
  try {
    return await Promise.race([tauriInvoke<T>(cmd, args), timeout]);
  } finally {
    clearTimeout(timer);
  }
}

export const api = {
  async profile(): Promise<Profile> {
    return invoke<Profile>("profile_get");
  },

  async startSession(kind: SessionKind): Promise<SessionStart> {
    return invoke<SessionStart>("session_start", { kind });
  },

  async submitRound(sessionId: number, roundId: number, result: RoundResult): Promise<RoundFeedback> {
    return invoke<RoundFeedback>("round_submit", { sessionId, roundId, result });
  },

  async finishSession(sessionId: number): Promise<SessionSummary> {
    return invoke<SessionSummary>("session_finish", { sessionId });
  },

  async placementStatus(): Promise<PlacementResult | null> {
    return invoke<PlacementResult | null>("placement_status");
  },

  async placementStart(): Promise<PlacementItem[]> {
    return invoke<PlacementItem[]>("placement_start");
  },

  async placementSubmit(answers: PlacementAnswer[]): Promise<PlacementResult> {
    return invoke<PlacementResult>("placement_submit", { answers });
  },

  async searchWords(query: string, limit = 50): Promise<WordHit[]> {
    return invoke<WordHit[]>("word_search", { query, limit });
  },

  /** null for a word id the dictionary doesn't have. */
  async wordDetail(wordId: number): Promise<WordDetail | null> {
    return invoke<WordDetail | null>("word_detail", { wordId });
  },

  async markWordKnown(wordId: number): Promise<void> {
    return invoke<void>("word_mark_known", { wordId });
  },

  async resetWord(wordId: number): Promise<void> {
    return invoke<void>("word_reset", { wordId });
  },

  /** The spoken text as a base64 wav, or null with no speech engine. Playing
   * it is the caller's business (see audio.ts, which can also stop it). */
  async ttsAudio(text: string): Promise<string | null> {
    return invoke<string | null>("tts_speak", { text });
  },

  async speak(text: string): Promise<void> {
    const b64 = await invoke<string | null>("tts_speak", { text });
    if (b64) {
      try {
        void new Audio(`data:audio/wav;base64,${b64}`).play();
      } catch {
        /* audio device unavailable */
      }
    }
  },

  async ttsInfo(): Promise<TtsInfo> {
    return invoke<TtsInfo>("tts_info");
  },

  async contentCredits(): Promise<string> {
    return invoke<string>("content_credits");
  },

  async importSrt(title: string, text: string): Promise<ImportReport> {
    return invoke<ImportReport>("srt_import", { title, text });
  },

  /** A page of mined sentences, newest first: the first page, or the one
   * after the sentence with id `beforeId` (the last one already shown). */
  async listSentences(beforeId?: number): Promise<SentencePage> {
    return invoke<SentencePage>("sentences_list", { beforeId });
  },

  async addSentenceToDeck(id: number): Promise<void> {
    return invoke<void>("sentence_add_to_deck", { id });
  },

  async stats(): Promise<StatsData> {
    return invoke<StatsData>("stats_get");
  },

  async getSettings(): Promise<Settings> {
    return invoke<Settings>("settings_get");
  },

  async setSettings(settings: Settings): Promise<Settings> {
    return invoke<Settings>("settings_set", { settings });
  },

  async listBadges(): Promise<Badge[]> {
    return invoke<Badge[]>("badges_list");
  },

  async backupFolder(): Promise<BackupFolder> {
    return invoke<BackupFolder>("backup_folder");
  },

  /** `null` goes back to the default folder. */
  async setBackupFolder(dir: string | null): Promise<BackupFolder> {
    return invoke<BackupFolder>("backup_folder_set", { dir });
  },

  async exportBackup(): Promise<string> {
    return invoke<string>("backup_export");
  },

  async importBackup(bytes: Uint8Array): Promise<void> {
    return invoke<void>("backup_import", { bytes: Array.from(bytes) });
  },

  /** Restores a backup listed by backupFolder(). */
  async restoreBackup(path: string): Promise<void> {
    return invoke<void>("backup_restore", { path });
  },

  async resetProgress(): Promise<void> {
    return invoke<void>("progress_reset");
  },
};
