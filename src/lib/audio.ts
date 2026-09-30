import { api } from "./api";

/**
 * Play round audio: prefer the provided base64 wav (data: URL),
 * otherwise fall back to TTS (backend under Tauri, speechSynthesis in browser).
 */
export function playRoundAudio(base64: string | null, fallbackText?: string): void {
  if (base64) {
    try {
      const el = new Audio(`data:audio/wav;base64,${base64}`);
      void el.play();
      return;
    } catch {
      /* fall through to TTS */
    }
  }
  if (fallbackText) void api.speak(fallbackText);
}
