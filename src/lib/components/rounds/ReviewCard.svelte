<script lang="ts">
  import { onMount } from "svelte";
  import type { Round } from "$lib/contract";
  import { playRoundAudio } from "$lib/audio";
  import { sfx } from "$lib/sfx";

  type RoundT = Extract<Round, { type: "review_card" }>;

  let {
    round,
    onAnswered,
  }: {
    round: RoundT;
    onAnswered: (correct: boolean, correctText?: string | null, quality?: number) => void;
  } = $props();

  let flipped = $state(false);

  const GRADES = [
    { q: 1, label: "Otra vez", emoji: "😶", style: "from-slate-500 to-slate-600" },
    { q: 2, label: "Difícil", emoji: "😅", style: "from-coral-500 to-rose-600" },
    { q: 3, label: "Bien", emoji: "🙂", style: "from-grape-500 to-fuchsia-600" },
    { q: 4, label: "Fácil", emoji: "😄", style: "from-lime-500 to-emerald-600" },
  ] as const;

  function grade(q: number) {
    sfx.pop();
    onAnswered(q >= 2, round.en, q);
  }

  function onKey(e: KeyboardEvent) {
    if (!flipped) {
      if (e.code === "Space" || e.key === "Enter") {
        e.preventDefault();
        flip();
      }
      return;
    }
    const n = parseInt(e.key, 10);
    if (n >= 1 && n <= 4) grade(GRADES[n - 1].q);
  }

  function flip() {
    flipped = true;
    sfx.pop();
  }

  onMount(() => {
    playRoundAudio(round.audioBase64, round.es);
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="mx-auto max-w-xl">
  <h1 class="mb-6 text-center text-3xl font-extrabold">Repaso flash 🔁</h1>

  <button
    class="card pressable block w-full p-10 text-center"
    onclick={flip}
    disabled={flipped}
    aria-label={flipped ? round.en : "Revelar significado"}
  >
    {#if !flipped}
      <p class="text-sm font-bold uppercase tracking-widest text-white/40">Toca para revelar</p>
      <p class="anim-pop mt-4 text-6xl font-black">{round.es}</p>
      <p class="mt-4 text-lg text-white/40">🔊 Escucha · Espacio o Enter para girar</p>
    {:else}
      <p class="anim-pop text-5xl font-black text-grape-400">{round.es}</p>
      <p class="anim-pop mt-3 text-3xl font-bold text-lime-500">{round.en}</p>
      {#if round.exampleEs}
        <p class="mt-4 text-lg italic text-white/60">"{round.exampleEs}"</p>
        {#if round.exampleEn}
          <p class="text-base text-white/40">{round.exampleEn}</p>
        {/if}
      {/if}
    {/if}
  </button>

  {#if flipped}
    <div class="anim-rise mt-6">
      <p class="mb-3 text-center text-base text-white/50">
        Sé honesto: tu memoria decide cuándo volver a verla 🧠
      </p>
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {#each GRADES as g, i (g.q)}
          <button
            class="pressable rounded-2xl bg-gradient-to-br {g.style} px-3 py-4 text-lg font-extrabold shadow-lg hover:brightness-110"
            onclick={() => grade(g.q)}
          >
            <span aria-hidden="true">{g.emoji}</span>
            {g.label}
            <span class="mt-0.5 block text-xs font-bold opacity-60">tecla {i + 1}</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>
