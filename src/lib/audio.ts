import { api } from "./api";
import { createClipPlayer, type Clip } from "./clipPlayer";

function start(base64: string): Clip | null {
  try {
    const el = new Audio(`data:audio/wav;base64,${base64}`);
    void el.play().catch((e: unknown) => {
      // pausing a clip before it has started rejects its play(): that was us
      if (!(e instanceof DOMException && e.name === "AbortError")) throw e;
    });
    return { stop: () => el.pause() };
  } catch {
    /* audio device unavailable */
    return null;
  }
}

const player = createClipPlayer({ tts: (text) => api.ttsAudio(text), start });

/**
 * Play round audio: prefer the provided base64 wav (data: URL),
 * otherwise fall back to backend TTS. Whatever was playing stops first.
 */
export function playRoundAudio(base64: string | null, fallbackText?: string): void {
  player.play(base64, fallbackText);
}

/** Stops round audio, and keeps speech that is still on its way from starting. */
export function stopAudio(): void {
  player.stop();
}
