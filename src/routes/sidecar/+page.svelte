<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { playRoundAudio } from "$lib/audio";
  import { sfx } from "$lib/sfx";
  import { grade } from "$lib/grading";
  import type { Round, SessionStart, SessionSummary } from "$lib/contract";

  let session = $state<SessionStart | null>(null);
  let idx = $state(0);
  let chosen = $state<number | null>(null);
  let typed = $state("");
  let phase = $state<"loading" | "playing" | "done">("loading");
  let summary = $state<SessionSummary | null>(null);
  let xp = $state(0);
  let roundStart = $state(0);
  let submitting = $state(false);

  const round = $derived(session?.rounds[idx] ?? null);
  const prompt = $derived.by(() => {
    if (!round) return "";
    if (round.type === "choice") return round.prompt;
    if (round.type === "listen") return "🎧 escucha";
    if (round.type === "listen_type") return "🎧 escucha y escribe";
    return "";
  });
  const options = $derived.by(() => {
    if (!round) return [] as string[];
    if (round.type === "choice" || round.type === "listen") return round.options;
    return [];
  });

  function playCurrent(r: Round | null = round) {
    if (!r) return;
    if (r.type === "listen") playRoundAudio(r.audioBase64, r.options[r.answerIndex]);
    else if (r.type === "listen_type") playRoundAudio(r.audioBase64, r.sentenceEs);
    else if (r.type === "choice" && r.promptLang === "es") playRoundAudio(null, r.prompt);
  }

  onMount(async () => {
    session = await api.startSession("sidecar");
    roundStart = performance.now();
    phase = "playing";
    playCurrent(session.rounds[0]);
  });

  async function answer(i: number) {
    if (!round || chosen !== null || submitting) return;
    submitting = true;
    chosen = i;
    const r = round;
    const correct = r.type === "choice" || r.type === "listen" ? i === r.answerIndex : false;
    if (correct) sfx.correct(0);
    else sfx.wrong();
    await submit(correct);
  }

  async function answerTyped(e: Event) {
    e.preventDefault();
    if (!round || round.type !== "listen_type" || submitting) return;
    submitting = true;
    const ok = grade(typed, round.answer) !== "wrong";
    if (ok) sfx.correct(0);
    else sfx.wrong();
    await submit(ok);
  }

  // `submitting` stays true until the next round is showing: a second Enter
  // during the 650 ms pause used to grade the round again and skip the next.
  async function submit(correct: boolean) {
    if (!session || !round) return;
    submitting = true;
    try {
      const feedback = await api.submitRound(session.sessionId, round.id, {
        roundIndex: idx,
        correct,
        durationMs: Math.round(performance.now() - roundStart),
      });
      xp += feedback.xpGained;
    } catch {
      submitting = false;
      chosen = null;
      return;
    }
    setTimeout(async () => {
      chosen = null;
      typed = "";
      if (idx + 1 >= session!.rounds.length) {
        summary = await api.finishSession(session!.sessionId);
        phase = "done";
      } else {
        idx += 1;
        roundStart = performance.now();
        submitting = false;
        playCurrent();
      }
    }, 650);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      history.back();
      return;
    }
    if (phase !== "playing") return;
    if (e.code === "Space") {
      e.preventDefault();
      playCurrent();
      return;
    }
    if (e.key === "Enter" && round?.type === "listen_type") {
      void answerTyped(e);
      return;
    }
    const n = parseInt(e.key, 10);
    if (n >= 1 && n <= 4 && options.length > 0) void answer(n - 1);
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="flex min-h-screen flex-col bg-black/95 px-8 py-6 text-white/90">
  {#if phase === "loading"}
    <p class="m-auto animate-pulse text-2xl text-white/40">cargando…</p>
  {:else if phase === "playing" && round}
    <div class="flex items-center gap-2">
      {#each session?.rounds ?? [] as _, i (i)}
        <span class="h-1.5 rounded-full {i < idx ? 'w-1.5 bg-lime-500/60' : i === idx ? 'w-5 bg-grape-400' : 'w-1.5 bg-white/15'}"></span>
      {/each}
      <span class="ml-auto text-sm text-white/30">modo secundario · Esc para salir</span>
    </div>

    <div class="flex flex-1 flex-col items-center justify-center gap-6 text-center">
      {#if round.type === "listen_type"}
        <form onsubmit={answerTyped} class="w-full max-w-lg">
          <input
            type="text"
            bind:value={typed}
            class="w-full rounded-2xl border border-white/15 bg-white/5 px-5 py-4 text-center text-2xl outline-none focus:border-grape-400"
            placeholder="escribe lo que oyes…"
            aria-label="Tu respuesta"
          />
        </form>
      {:else}
        <p class="text-5xl font-black tracking-tight">{prompt}</p>
      {/if}
      <button
        class="rounded-full bg-white/10 px-5 py-2 text-lg hover:bg-white/20"
        onclick={() => playCurrent()}
        aria-label="Reproducir audio de nuevo"
      >
        🔊 otra vez <span class="text-white/40">(Espacio)</span>
      </button>
    </div>

    {#if options.length > 0}
      <div class="grid grid-cols-2 gap-3">
        {#each options as opt, i (i)}
          <button
            class="rounded-2xl px-5 py-6 text-2xl font-bold transition-all
              {chosen === null
              ? 'bg-white/5 hover:bg-white/12'
              : i === (round.type === 'choice' || round.type === 'listen' ? round.answerIndex : -1)
                ? 'bg-lime-500/30 text-lime-400'
                : i === chosen
                  ? 'bg-coral-500/30'
                  : 'opacity-30'}"
            onclick={() => answer(i)}
          >
            <span class="mr-2 text-base font-black text-white/35" aria-hidden="true">{i + 1}</span>{opt}
          </button>
        {/each}
      </div>
    {/if}
  {:else if phase === "done" && summary}
    <div class="m-auto text-center">
      <p class="text-4xl font-black">¡Hecho! ⚡ {summary.xp} XP</p>
      <p class="mt-2 text-lg text-white/50">{summary.correct}/{summary.rounds} aciertos · Esc o </p>
      <button class="mt-4 rounded-full bg-grape-500 px-6 py-2 text-lg font-bold" onclick={() => goto("/")}>
        volver
      </button>
    </div>
  {/if}
</div>
