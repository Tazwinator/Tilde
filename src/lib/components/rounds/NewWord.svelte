<script lang="ts">
  import type { Round } from "$lib/contract";
  import { glossText } from "$lib/defs";
  import { playRoundAudio } from "$lib/audio";
  import { sfx } from "$lib/sfx";

  type RoundT = Extract<Round, { type: "new_word" }>;

  let {
    round,
    definitionLang,
    onAnswered,
  }: {
    round: RoundT;
    definitionLang: string;
    onAnswered: (correct: boolean, correctText?: string | null) => void;
  } = $props();

  const gloss = $derived(glossText(round.word, definitionLang));
</script>

<div class="card anim-pop mx-auto max-w-xl p-8 text-center">
  <p class="text-lg font-bold uppercase tracking-widest text-tubo-500">✨ Palabra nueva</p>
  <h1 class="anim-pop my-5 text-6xl font-black tracking-tight">{round.word.lemma}</h1>
  {#if round.word.pos}
    <p class="mb-2 text-sm font-semibold uppercase text-white/40">{round.word.pos} · {round.word.level}</p>
  {/if}
  <p class="text-2xl font-bold text-grape-400">{gloss}</p>

  {#if round.exampleEs}
    <div class="mt-6 rounded-2xl bg-white/5 p-4">
      <div class="flex items-center justify-center gap-2">
        <button
          class="pressable rounded-full bg-white/10 px-3 py-1 text-sm font-bold hover:bg-white/20"
          aria-label="Escuchar ejemplo"
          onclick={() => playRoundAudio(round.audioBase64, round.exampleEs ?? undefined)}
        >
          🔊
        </button>
        <p class="text-lg italic">"{round.exampleEs}"</p>
      </div>
      {#if round.exampleEn}
        <p class="mt-1 text-base text-white/55">{round.exampleEn}</p>
      {/if}
    </div>
  {/if}

  <button
    class="pressable mt-8 w-full rounded-3xl bg-gradient-to-r from-lime-500 to-emerald-500 py-4 text-2xl font-extrabold text-night-900 shadow-xl shadow-lime-500/30 hover:brightness-110"
    onclick={() => {
      sfx.pop();
      onAnswered(true, null);
    }}
  >
    ¡Lo tengo! 💪
  </button>
</div>
