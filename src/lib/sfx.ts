// Hand-rolled WebAudio synth SFX — no assets.
// All sounds are short (<0.6s) oscillator + gain envelope blips.

const LS_KEY = "tilde.sound.enabled";

let ctx: AudioContext | null = null;

function audioCtx(): AudioContext | null {
  try {
    if (!ctx) {
      const AC =
        window.AudioContext ??
        (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!AC) return null;
      ctx = new AC();
    }
    if (ctx.state === "suspended") void ctx.resume();
    return ctx;
  } catch {
    return null;
  }
}

function loadEnabled(): boolean {
  try {
    return localStorage.getItem(LS_KEY) !== "0";
  } catch {
    return true;
  }
}

let enabled = typeof localStorage !== "undefined" ? loadEnabled() : true;

export function sfxEnabled(): boolean {
  return enabled;
}

export function setSfxEnabled(v: boolean): void {
  enabled = v;
  try {
    localStorage.setItem(LS_KEY, v ? "1" : "0");
  } catch {
    /* ignore */
  }
}

interface ToneOpts {
  freq: number;
  dur?: number;
  type?: OscillatorType;
  gain?: number;
  delay?: number;
  glideTo?: number;
}

function tone({ freq, dur = 0.12, type = "sine", gain = 0.16, delay = 0, glideTo }: ToneOpts): void {
  const c = audioCtx();
  if (!c) return;
  const t0 = c.currentTime + delay;
  const osc = c.createOscillator();
  const g = c.createGain();
  osc.type = type;
  osc.frequency.setValueAtTime(freq, t0);
  if (glideTo !== undefined) osc.frequency.exponentialRampToValueAtTime(Math.max(40, glideTo), t0 + dur);
  g.gain.setValueAtTime(0.0001, t0);
  g.gain.exponentialRampToValueAtTime(gain, t0 + 0.012);
  g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
  osc.connect(g).connect(c.destination);
  osc.start(t0);
  osc.stop(t0 + dur + 0.05);
}

function noise(dur = 0.14, gain = 0.07, delay = 0): void {
  const c = audioCtx();
  if (!c) return;
  const t0 = c.currentTime + delay;
  const buf = c.createBuffer(1, Math.ceil(c.sampleRate * dur), c.sampleRate);
  const data = buf.getChannelData(0);
  for (let i = 0; i < data.length; i++) data[i] = (Math.random() * 2 - 1) * (1 - i / data.length);
  const src = c.createBufferSource();
  src.buffer = buf;
  const g = c.createGain();
  g.gain.setValueAtTime(gain, t0);
  g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
  const filter = c.createBiquadFilter();
  filter.type = "lowpass";
  filter.frequency.value = 1100;
  src.connect(filter).connect(g).connect(c.destination);
  src.start(t0);
}

export const sfx = {
  click(): void {
    if (!enabled) return;
    tone({ freq: 620, dur: 0.06, type: "triangle", gain: 0.08 });
  },

  pop(): void {
    if (!enabled) return;
    tone({ freq: 300, dur: 0.1, type: "sine", gain: 0.14, glideTo: 640 });
  },

  /** correct answer; pitch rises with combo level */
  correct(combo = 0): void {
    if (!enabled) return;
    const step = Math.min(combo, 8) * 42;
    tone({ freq: 523 + step, dur: 0.1, type: "sine", gain: 0.14 });
    tone({ freq: 659 + step, dur: 0.12, type: "sine", gain: 0.13, delay: 0.07 });
    tone({ freq: 784 + step, dur: 0.16, type: "sine", gain: 0.12, delay: 0.14 });
  },

  /** soft low buzz — gentle, not harsh */
  wrong(): void {
    if (!enabled) return;
    tone({ freq: 196, dur: 0.22, type: "sine", gain: 0.12, glideTo: 130 });
    tone({ freq: 155, dur: 0.2, type: "triangle", gain: 0.05, delay: 0.04 });
  },

  levelUp(): void {
    if (!enabled) return;
    const notes = [523, 659, 784, 1047];
    notes.forEach((f, i) => tone({ freq: f, dur: 0.14, type: "triangle", gain: 0.13, delay: i * 0.09 }));
    tone({ freq: 1319, dur: 0.28, type: "sine", gain: 0.1, delay: 0.38 });
  },

  badge(): void {
    if (!enabled) return;
    tone({ freq: 988, dur: 0.09, type: "sine", gain: 0.1 });
    tone({ freq: 1319, dur: 0.14, type: "sine", gain: 0.09, delay: 0.08 });
    noise(0.18, 0.03, 0.06);
  },

  fanfare(): void {
    if (!enabled) return;
    const notes = [392, 523, 659, 784, 659, 784, 1047];
    notes.forEach((f, i) => tone({ freq: f, dur: 0.13, type: "triangle", gain: 0.12, delay: i * 0.1 }));
  },
};
