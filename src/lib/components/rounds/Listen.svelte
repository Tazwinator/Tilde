<script lang="ts">
  import { onMount } from "svelte";
  import type { Round } from "$lib/contract";
  import { playRoundAudio } from "$lib/audio";
  import { sfx } from "$lib/sfx";

  type RoundT = Extract<Round, { type: "listen" }>;

  let {
    round,
    onAnswered,
  }: {
    round: RoundT;
    onAnswered: (correct: boolean, correctText?: string | null) => void;
  } = $props();

  let chosen = $state<number | null>(null);
  const answerText = $derived(round.options[round.answerIndex]);

  function play() {
    playRoundAudio(round.audioBase64, answerText);
  }

  onMount(play);

  function pick(i: number) {
    if (chosen !== null) return;
    chosen = i;
    const correct = i === round.answerIndex;
    if (correct) sfx.correct(0);
    else sfx.wrong();
    setTimeout(() => onAnswered(correct, answerText), 700);
  }

  function onKey(e: KeyboardEvent) {
    if (e.code === "Space") {
      e.preventDefault();
      play();
      return;
    }
    if (chosen !== null) return;
    const n = parseInt(e.key, 10);
    if (n >= 1 && n <= 4) pick(n - 1);
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="mx-auto max-w-xl">
  <h1 class="mb-6 text-center text-3xl font-extrabold">Escucha y elige 🎧</h1>
  <div class="mb-6 flex justify-center">
    <button
      class="pressable flex h-24 w-24 items-center justify-center rounded-full bg-gradient-to-br from-tubo-500 to-grape-500 text-4xl shadow-xl shadow-tubo-500/30 hover:brightness-110"
      aria-label="Reproducir audio"
      onclick={play}
    >
      🔊
    </button>
  </div>
  <div class="grid grid-cols-2 gap-4">
    {#each round.options as opt, i (i)}
      <button
        class="min-h-20 rounded-3xl border-2 px-5 py-4 text-2xl font-bold transition-all
          {chosen === null
          ? 'border-white/10 bg-white/5 hover:border-grape-400/60 hover:bg-white/10'
          : i === round.answerIndex
            ? 'border-lime-500 bg-lime-500/25'
            : i === chosen
              ? 'anim-shake border-coral-500 bg-coral-500/25'
              : 'border-white/5 bg-white/[0.02] opacity-40'}"
        disabled={chosen !== null}
        onclick={() => pick(i)}
      >
        <span class="mr-2 text-sm font-black text-white/40" aria-hidden="true">{i + 1}</span>{opt}
      </button>
    {/each}
  </div>
</div>
