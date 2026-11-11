/**
 * Which round clip may sound (see audio.ts). One at a time, and none that
 * arrives late. No runtime imports, so `node --test` can load it.
 */

export interface Clip {
  stop(): void;
}

export interface ClipPlayerDeps {
  /** Speech for a text as a base64 wav, or null with no speech engine. */
  tts(text: string): Promise<string | null>;
  /** Starts a clip; null when it can't be played. */
  start(base64: string): Clip | null;
}

export function createClipPlayer(deps: ClipPlayerDeps) {
  // A new clip or stop() replaces it, so replaying with Space can't stack
  // the same word on top of itself.
  let current: Clip | null = null;
  // Goes up with every play and stop. Speech still being made when the
  // player has moved on finds a newer number and stays silent.
  let generation = 0;

  function stop(): void {
    generation += 1;
    current?.stop();
    current = null;
  }

  /** Plays the given wav, or else asks for speech of `fallbackText`. */
  function play(base64: string | null, fallbackText?: string): void {
    stop();
    if (base64) {
      current = deps.start(base64);
      return;
    }
    if (!fallbackText) return;
    const mine = generation;
    void deps.tts(fallbackText).then((b64) => {
      if (b64 && mine === generation) current = deps.start(b64);
    });
  }

  return { play, stop };
}
