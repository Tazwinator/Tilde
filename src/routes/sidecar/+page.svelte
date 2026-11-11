<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { beforeNavigate, goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { playRoundAudio, stopAudio } from "$lib/audio";
  import { badgeSound, celebrate } from "$lib/celebrate.svelte";
  import { sfx } from "$lib/sfx";
  import { grade, type Verdict } from "$lib/grading";
  import { textSize } from "$lib/defs";
  import { typingInto } from "$lib/keys";
  import { reportError } from "$lib/errors.svelte";
  import { createFlow, initialView, mergeBadges, SIDECAR_REVEAL_MS, type FlowView } from "$lib/sessionFlow";
  import type { Round } from "$lib/contract";

  // The same session flow as the play page (sessionFlow.ts), with a screen
  // made for the corner of the eye. `view` is written by the flow: always
  // read it as `view.x`, never destructure it.
  const view = $state<FlowView>(initialView());
  const flow = createFlow(
    view,
    {
      // no getSettings: the kind is fixed and nothing here shows a definition
      api: {
        startSession: (kind) => api.startSession(kind),
        submitRound: (sessionId, roundId, result) => api.submitRound(sessionId, roundId, result),
        finishSession: (sessionId) => api.finishSession(sessionId),
      },
      clock: {
        setTimeout: (fn, ms) => setTimeout(fn, ms),
        clearTimeout: (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
        now: () => performance.now(),
      },
      celebrate,
      reportError,
      // the done screen lists them, so no toast: just the sound
      onDone: (summary) => {
        if (summary.newBadges.length > 0) badgeSound();
      },
      revealMs: () => SIDECAR_REVEAL_MS,
    },
    { requested: null, fixedKind: "sidecar" },
  );

  onMount(() => void flow.start());

  // any way out of here ends the session; nothing may sound afterwards
  function leave() {
    flow.leave();
    stopAudio();
  }
  beforeNavigate(leave);
  onDestroy(leave);

  // the answer given to the round on screen
  let chosen = $state<number | null>(null);
  let typed = $state("");
  let verdict = $state<Verdict | null>(null);
  let dictation = $state<HTMLInputElement>();

  const round = $derived(view.phase === "playing" ? (view.session?.rounds[view.idx] ?? null) : null);
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
  const badges = $derived(mergeBadges(view.roundBadges, view.summary?.newBadges ?? []));

  function playCurrent(r: Round | null = round) {
    if (!r) return;
    if (r.type === "listen") playRoundAudio(r.audioBase64, r.es);
    else if (r.type === "listen_type") playRoundAudio(r.audioBase64, r.sentenceEs);
    else if (r.type === "choice" && r.promptLang === "es") playRoundAudio(null, r.prompt);
  }

  // A round has come on screen (the flow decides when): a clean slate, and
  // its audio. Depends on the round alone.
  $effect(() => {
    const r = round;
    untrack(() => {
      chosen = null;
      typed = "";
      verdict = null;
      playCurrent(r);
    });
  });

  // straight into the box: sidecar is meant to be answered without the mouse
  $effect(() => {
    if (round?.type === "listen_type") dictation?.focus();
  });

  function answer(i: number) {
    if (!round || (round.type !== "choice" && round.type !== "listen") || chosen !== null) return;
    chosen = i;
    const correct = i === round.answerIndex;
    if (correct) sfx.correct(0);
    else sfx.wrong();
    void flow.answer(correct, round.options[round.answerIndex]);
  }

  function answerTyped() {
    if (!round || round.type !== "listen_type" || verdict !== null) return;
    // hearing a sentence doesn't tell you its accents: count them, but show the spelling
    verdict = grade(typed, round.answer);
    const ok = verdict !== "wrong";
    if (ok) sfx.correct(0);
    else sfx.wrong();
    // Only an exact answer moves on by itself. A miss or a missing accent
    // shows the sentence, and that takes longer to read than the reveal
    // lasts: the round stays until Enter.
    void flow.answer(ok, round.answer, undefined, undefined, verdict !== "right");
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      // not history.back(): that goes nowhere when this was the first page
      void goto("/");
      return;
    }
    if (!round) return;
    // nothing is being typed into a box that has been answered
    const typing = typingInto(e) && verdict === null;
    // Ctrl+Space replays from anywhere; plain Space does too, unless it's
    // a space being typed into the dictation box
    if (e.code === "Space" && (e.ctrlKey || !typing)) {
      e.preventDefault();
      playCurrent();
      return;
    }
    if (round.type === "listen_type") {
      if (e.key !== "Enter" || e.isComposing) return;
      e.preventDefault();
      // an Enter held down must not answer the round and then wave the
      // answer through unread
      if (e.repeat) return;
      if (view.held) flow.continue();
      else answerTyped();
      return;
    }
    const n = parseInt(e.key, 10);
    if (n >= 1 && n <= 4 && options.length > 0) answer(n - 1);
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="flex min-h-screen flex-col bg-black/95 px-8 py-6 text-white/90">
  {#if view.phase === "loading"}
    <p class="m-auto animate-pulse text-2xl text-white/40">cargando…</p>
  {:else if view.phase === "empty" || view.phase === "error"}
    <div class="m-auto max-w-lg text-center">
      <p class="text-2xl font-bold">
        {#if view.phase === "empty"}
          No hay palabras para repasar todavía. Juega una sesión normal primero.
        {:else if view.session}
          No se ha podido cerrar la sesión. Tus respuestas ya estaban guardadas.
        {:else}
          No se ha podido preparar la sesión.
        {/if}
      </p>
      <p class="mt-2 text-lg text-white/50">Esc para salir</p>
      <button class="mt-4 rounded-full bg-grape-500 px-6 py-2 text-lg font-bold" onclick={() => goto("/")}>
        volver
      </button>
    </div>
  {:else if view.phase === "playing" && round}
    <div class="flex items-center gap-2">
      {#each view.session?.rounds ?? [] as _, i (i)}
        <span class="h-1.5 rounded-full {i < view.idx ? 'w-1.5 bg-lime-500/60' : i === view.idx ? 'w-5 bg-grape-400' : 'w-1.5 bg-white/15'}"></span>
      {/each}
      <span class="ml-auto text-sm text-white/30">modo secundario · Esc para salir</span>
    </div>

    <div class="flex flex-1 flex-col items-center justify-center gap-6 text-center">
      {#if round.type === "listen_type"}
        <div class="w-full max-w-lg">
          <!-- read-only once answered, not disabled: it keeps the focus -->
          <input
            bind:this={dictation}
            type="text"
            bind:value={typed}
            readonly={verdict !== null}
            class="w-full rounded-2xl border bg-white/5 px-5 py-4 text-center text-2xl outline-none
              {verdict === null ? 'border-white/15 focus:border-grape-400' : verdict === 'wrong' ? 'border-coral-500' : 'border-lime-500'}"
            placeholder="escribe lo que oyes…"
            aria-label="Tu respuesta"
          />
          {#if verdict === "wrong"}
            <p class="mt-3 text-xl text-white/70">era: <strong class="text-lime-500">{round.answer}</strong></p>
          {:else if verdict === "accents"}
            <p class="mt-3 text-xl text-white/70">con sus tildes: <strong class="text-lime-500">{round.answer}</strong></p>
          {/if}
          {#if view.held}
            <p class="mt-2 text-sm text-white/40">Enter para seguir</p>
          {/if}
        </div>
      {:else}
        <p class="text-5xl font-black tracking-tight">{prompt}</p>
      {/if}
      <button
        class="rounded-full bg-white/10 px-5 py-2 text-lg hover:bg-white/20"
        onclick={() => {
          playCurrent();
          // a click takes the focus; typing goes on in the box
          dictation?.focus();
        }}
        aria-label="Reproducir audio de nuevo"
      >
        🔊 otra vez <span class="text-white/40">({round.type === "listen_type" ? "Ctrl+Espacio" : "Espacio"})</span>
      </button>
    </div>

    {#if options.length > 0}
      <div class="grid grid-cols-2 gap-3">
        {#each options as opt, i (i)}
          <button
            class="rounded-2xl px-5 py-6 {textSize(opt)} font-bold transition-all
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
  {:else if view.phase === "done" && view.summary}
    <div class="m-auto text-center">
      <p class="text-4xl font-black">¡Hecho! ⚡ {view.summary.xp} XP</p>
      <!-- the button is part of the sentence, which must not break before it -->
      <p class="mt-2 whitespace-nowrap text-lg text-white/50">
        {view.summary.correct}/{view.summary.rounds} aciertos · pulsa Esc o
        <button class="rounded-full bg-grape-500 px-4 py-1 font-bold text-white/90" onclick={() => goto("/")}>volver</button>
      </p>
      {#each badges as b (b.id)}
        <p class="mt-3 text-lg">
          <span aria-hidden="true">🏅</span>
          <span class="font-extrabold text-mango-500">{b.name}</span>
          <span class="text-white/50">· {b.description}</span>
        </p>
      {/each}
    </div>
  {/if}
</div>
