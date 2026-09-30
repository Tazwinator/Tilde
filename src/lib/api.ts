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

// Thin typed wrapper over the Tauri commands in src-tauri/src/lib.rs.
// Run the UI with `npm run tauri dev`; there is no browser-only backend.

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

  async placementSubmit(answers: PlacementAnswer[], _startBand?: string): Promise<PlacementResult> {
    return invoke<PlacementResult>("placement_submit", { answers, startBand: _startBand ?? null });
  },

  async searchWords(query: string, limit = 50): Promise<WordHit[]> {
    return invoke<WordHit[]>("word_search", { query, limit });
  },

  async wordDetail(wordId: number): Promise<WordDetail> {
    return invoke<WordDetail>("word_detail", { wordId });
  },

  async markWordKnown(wordId: number): Promise<void> {
    return invoke<void>("word_mark_known", { wordId });
  },

  async resetWord(wordId: number): Promise<void> {
    return invoke<void>("word_reset", { wordId });
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

  async listSentences(): Promise<MinedSentence[]> {
    return invoke<MinedSentence[]>("sentences_list");
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

  async exportBackup(): Promise<string> {
    return invoke<string>("backup_export");
  },

  async importBackup(bytes: Uint8Array): Promise<void> {
    return invoke<void>("backup_import", { bytes: Array.from(bytes) });
  },

  async resetProgress(): Promise<void> {
    return invoke<void>("progress_reset");
  },
};
