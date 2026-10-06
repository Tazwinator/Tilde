<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { api } from "$lib/api";
  import { confetti } from "$lib/confetti";
  import { sfx } from "$lib/sfx";
  import { reportError } from "$lib/errors.svelte";
  import type { RoundFeedback, SessionKind, SessionStart, SessionSummary } from "$lib/contract";
  import Modal from "$lib/components/Modal.svelte";
  import NewWord from "$lib/components/rounds/NewWord.svelte";
  import Choice from "$lib/components/rounds/Choice.svelte";
  import Match from "$lib/components/rounds/Match.svelte";
  import Listen from "$lib/components/rounds/Listen.svelte";
  import ListenType from "$lib/components/rounds/ListenType.svelte";
  import Build from "$lib/components/rounds/Build.svelte";
  import Cloze from "$lib/components/rounds/Cloze.svelte";
  import Conjugation from "$lib/components/rounds/Conjugation.svelte";
  import ReviewCard from "$lib/components/rounds/ReviewCard.svelte";

  const PRAISE = ["¡Bien!", "¡Perfecto!", "¡Excelente!", "¡Genial!", "¡Muy bien!", "¡Brutal!"];

  const kind = $derived((page.url.searchParams.get("kind") ?? "standard") as SessionKind);

  let session = $state<SessionStart | null>(null);
  let idx = $state(0);
  let xp = $state(0);
  let combo = $state(0);
  let definitionLang = $state("en");
  let phase = $state<"loading" | "playing" | "done">("loading");
  let flash = $state<{ correct: boolean; text: string } | null>(null);
  let summary = $state<SessionSummary | null>(null);
  let showExit = $state(false);
  let submitting = $state(false);
  let roundStart = $state(0);

  $effect(() => {
    void (async () => {
      const [p, s] = await Promise.all([api.profile(), api.startSession(kind)]);
      definitionLang = p.definitionLang;
      session = s;
      roundStart = performance.now();
      phase = "playing";
    })();
  });

  const round = $derived(session?.rounds[idx] ?? null);
  const progress = $derived(session ? idx / session.rounds.length : 0);

  // `submitting` stays true from an answer until the next round is on screen
  // (or for good once the last one is in), so a double click or a late
  // callback can't grade a round twice or finish the session twice.
  async function handleAnswered(correct: boolean, correctText?: string | null, quality?: number, missedWordIds?: number[]) {
    if (!session || submitting || !round) return;
    submitting = true;
    const durationMs = Math.round(performance.now() - roundStart);
    let feedback: RoundFeedback;
    try {
      feedback = await api.submitRound(session.sessionId, round.id, {
        roundIndex: idx,
        correct,
        durationMs,
        quality: quality ?? null,
        missedWordIds: missedWordIds ?? [],
      });
    } catch (e) {
      reportError(e);
      submitting = false;
      return;
    }
    xp += feedback.xpGained;
    combo = feedback.combo;

    if (feedback.levelUp) {
      sfx.levelUp();
      confetti(window.innerWidth / 2, window.innerHeight * 0.3, 140);
    } else if (combo > 0 && combo % 5 === 0) {
      confetti(window.innerWidth / 2, window.innerHeight * 0.35, 90);
    }

    const praise = PRAISE[(Math.random() * PRAISE.length) | 0];
    flash = correct
      ? { correct: true, text: feedback.levelUp ? `¡NIVEL ${feedback.levelUp.level}! ${feedback.levelUp.name}` : praise }
      : { correct: false, text: correctText ? `Casi — era «${correctText}»` : "Casi — ¡sigue así!" };
    setTimeout(() => (flash = null), correct ? 900 : 1400);

    if (idx + 1 >= session.rounds.length) {
      await finish();
    } else {
      idx += 1;
      roundStart = performance.now();
      submitting = false;
    }
  }

  async function finish() {
    if (!session) return;
    summary = await api.finishSession(session.sessionId);
    phase = "done";
    if (summary.accuracy >= 0.8) {
      confetti(window.innerWidth / 2, window.innerHeight * 0.3, 160);
      sfx.fanfare();
    }
  }

  async function exitSession() {
    showExit = false;
    if (session) {
      try {
        await api.finishSession(session.sessionId);
      } catch {
        /* session may already be finished */
      }
    }
    goto("/");
  }

  function accuracyColor(a: number): string {
    if (a >= 0.8) return "#34d399";
    if (a >= 0.6) return "#fbbf24";
    return "#fb7185";
  }
</script>

{#if phase === "loading"}
  <div class="flex h-[70vh] items-center justify-center">
    <div class="anim-float text-center">
      <div class="text-6xl" aria-hidden="true">🎴</div>
      <p class="mt-3 text-xl font-bold text-white/60">Preparando la mesa…</p>
    </div>
  </div>
{:else if phase === "playing" && session && session.rounds.length === 0}
  <div class="card mx-auto flex max-w-xl flex-col items-center gap-4 p-10 text-center">
    <p class="text-5xl" aria-hidden="true">🌿</p>
    <h1 class="text-3xl font-extrabold">Nada que repasar ahora mismo</h1>
    <p class="text-lg text-white/60">Tus palabras están frescas. ¿Una sesión normal con alguna nueva?</p>
    <div class="mt-2 flex w-full gap-3">
      <button
        class="pressable flex-1 rounded-2xl bg-gradient-to-r from-grape-500 to-fuchsia-500 py-3 text-lg font-extrabold hover:brightness-110"
        onclick={() => void goto("/play?kind=standard")}
      >
        🎯 Sesión normal
      </button>
      <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-extrabold hover:bg-white/20" onclick={() => void goto("/")}>
        🏠 Inicio
      </button>
    </div>
  </div>
{:else if phase === "playing" && session}
  <div class="relative mx-auto max-w-3xl">
    <!-- Top bar -->
    <div class="mb-8 flex items-center gap-4">
      <button
        class="pressable flex h-10 w-10 items-center justify-center rounded-full bg-white/10 text-xl font-black hover:bg-white/20"
        aria-label="Salir de la sesión"
        onclick={() => {
          sfx.click();
          showExit = true;
        }}
      >
        ✕
      </button>
      <div class="flex flex-1 items-center gap-1.5" aria-label="Progreso de la sesión">
        {#each session.rounds as _, i (i)}
          <span
            class="h-2 rounded-full transition-all duration-300
              {i < idx ? 'w-2 bg-lime-500/70' : i === idx ? 'w-6 bg-grape-400' : 'w-2 bg-white/15'}"
          ></span>
        {/each}
      </div>
      {#if combo > 1}
        <span
          class="anim-glow rounded-full bg-orange-500/20 px-3.5 py-1.5 text-lg font-black text-orange-400"
          class:anim-glow={combo >= 3}
        >
          🔥 x{combo}
        </span>
      {/if}
      <span class="rounded-full bg-grape-500/20 px-3.5 py-1.5 text-lg font-black text-grape-400">⚡ {xp} XP</span>
    </div>

    <!-- Round -->
    {#key idx}
      <div class="anim-rise">
        {#if round}
          {#if round.type === "new_word"}
            <NewWord round={round} {definitionLang} onAnswered={handleAnswered} />
          {:else if round.type === "choice"}
            <Choice round={round} onAnswered={handleAnswered} />
          {:else if round.type === "match"}
            <Match round={round} onAnswered={handleAnswered} />
          {:else if round.type === "listen"}
            <Listen round={round} onAnswered={handleAnswered} />
          {:else if round.type === "listen_type"}
            <ListenType round={round} onAnswered={handleAnswered} />
          {:else if round.type === "build"}
            <Build round={round} onAnswered={handleAnswered} />
          {:else if round.type === "cloze"}
            <Cloze round={round} onAnswered={handleAnswered} />
          {:else if round.type === "conjugation"}
            <Conjugation round={round} onAnswered={handleAnswered} />
          {:else if round.type === "review_card"}
            <ReviewCard round={round} onAnswered={handleAnswered} />
          {/if}
        {/if}
      </div>
    {/key}

    <!-- Feedback flash -->
    {#if flash}
      <div class="pointer-events-none fixed inset-x-0 bottom-10 z-50 flex justify-center">
        <div
          class="anim-pop rounded-full px-8 py-3.5 text-2xl font-black shadow-2xl
            {flash.correct ? 'bg-lime-500 text-night-900' : 'bg-coral-500 text-white'}"
          role="status"
        >
          {flash.text}
        </div>
      </div>
    {/if}

    <!-- Exit confirm -->
    <Modal open={showExit} title="¿Salir de la sesión?" onclose={() => (showExit = false)}>
      <p class="text-lg text-white/70">
        Tu progreso hasta ahora queda guardado. Aquí no se pierde nada — vuelve cuando quieras.
      </p>
      <div class="mt-6 flex gap-3">
        <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-bold hover:bg-white/20" onclick={() => (showExit = false)}>
          Seguir jugando
        </button>
        <button class="pressable flex-1 rounded-2xl bg-coral-500 py-3 text-lg font-extrabold text-white hover:brightness-110" onclick={exitSession}>
          Salir
        </button>
      </div>
    </Modal>
  </div>
{:else if phase === "done" && summary}
  <!-- Session summary -->
  <div class="mx-auto flex max-w-2xl flex-col items-center gap-6">
    <h1 class="anim-pop text-4xl font-extrabold">¡Sesión completada! 🎊</h1>

    <div class="card flex w-full flex-col items-center p-8">
      <svg width="180" height="180" viewBox="0 0 120 120" role="img" aria-label="Precisión {Math.round(summary.accuracy * 100)}%">
        <circle cx="60" cy="60" r="50" fill="none" stroke="rgba(255,255,255,0.08)" stroke-width="12" />
        <circle
          cx="60"
          cy="60"
          r="50"
          fill="none"
          stroke={accuracyColor(summary.accuracy)}
          stroke-width="12"
          stroke-linecap="round"
          stroke-dasharray="{(summary.accuracy * 2 * Math.PI * 50).toFixed(1)} 999"
          transform="rotate(-90 60 60)"
          style="transition: stroke-dasharray 1s ease"
        />
        <text x="60" y="66" text-anchor="middle" font-size="28" font-weight="800" fill="#fff">{Math.round(summary.accuracy * 100)}%</text>
        <text x="60" y="84" text-anchor="middle" font-size="10" fill="rgba(255,255,255,0.5)">precisión</text>
      </svg>

      <div class="mt-6 grid w-full grid-cols-3 gap-3 text-center">
        <div class="rounded-2xl bg-white/5 p-4">
          <p class="text-3xl font-black text-grape-400">⚡ {summary.xp}</p>
          <p class="text-sm text-white/50">XP ganada</p>
        </div>
        <div class="rounded-2xl bg-white/5 p-4">
          <p class="text-3xl font-black text-orange-400">🔥 {summary.bestCombo}</p>
          <p class="text-sm text-white/50">mejor combo</p>
        </div>
        <div class="rounded-2xl bg-white/5 p-4">
          <p class="text-3xl font-black text-tubo-500">⏱ {summary.minutes < 1 ? "<1" : Math.round(summary.minutes)}</p>
          <p class="text-sm text-white/50">minutos</p>
        </div>
      </div>

      {#if summary.newWords.length > 0}
        <div class="mt-6 w-full">
          <p class="mb-2 text-center text-lg font-bold text-white/70">Palabras nuevas en tu colección ✨</p>
          <div class="flex flex-wrap justify-center gap-2">
            {#each summary.newWords as w (w)}
              <span class="anim-pop rounded-full bg-lime-500/20 px-4 py-1.5 text-lg font-bold text-lime-500">{w}</span>
            {/each}
          </div>
        </div>
      {/if}

      {#if summary.levelUp}
        <div class="anim-pop mt-6 rounded-3xl bg-gradient-to-r from-mango-500 to-amber-500 px-8 py-4 text-center text-night-900 shadow-xl shadow-mango-500/30">
          <p class="text-2xl font-black">🎉 ¡Nivel {summary.levelUp.level}!</p>
          <p class="text-lg font-bold">{summary.levelUp.name}</p>
        </div>
      {/if}

      {#each summary.newBadges as b (b.id)}
        <div class="anim-pop mt-4 flex items-center gap-3 rounded-2xl border border-mango-500/50 bg-mango-500/10 px-6 py-3">
          <span class="anim-sparkle text-3xl" aria-hidden="true">🏅</span>
          <div>
            <p class="font-extrabold text-mango-500">{b.name}</p>
            <p class="text-sm text-white/60">{b.description}</p>
          </div>
        </div>
      {/each}
    </div>

    <div class="flex w-full gap-3">
      <button
        class="pressable flex-1 rounded-3xl bg-gradient-to-r from-grape-500 to-fuchsia-500 py-4 text-xl font-extrabold shadow-lg shadow-grape-500/30 hover:brightness-110"
        onclick={async () => {
          sfx.click();
          phase = "loading";
          summary = null;
          idx = 0;
          xp = 0;
          combo = 0;
          submitting = false;
          session = await api.startSession(kind);
          roundStart = performance.now();
          phase = "playing";
        }}
      >
        🔁 Otra
      </button>
      <button
        class="pressable flex-1 rounded-3xl bg-white/10 py-4 text-xl font-extrabold hover:bg-white/20"
        onclick={() => {
          sfx.click();
          goto("/");
        }}
      >
        🏠 Inicio
      </button>
    </div>
  </div>
{/if}
