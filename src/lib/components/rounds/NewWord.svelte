<script lang="ts">
  import { onMount } from "svelte";
  import type { Round } from "$lib/contract";
  import { glossText, posLabel } from "$lib/defs";
  import { playRoundAudio } from "$lib/audio";
  import { activates } from "$lib/keys";
  import { KNOWN_ALREADY } from "$lib/sessionFlow";
  import { sfx } from "$lib/sfx";

  type RoundT = Extract<Round, { type: "new_word" }>;

  let {
    round,
    definitionLang,
    onAnswered,
  }: {
    round: RoundT;
    definitionLang: string;
    onAnswered: (correct: boolean, correctText?: string | null, quality?: number) => void;
  } = $props();

  const gloss = $derived(glossText(round.word, definitionLang));

  // the round's audio is the word itself; hear it as the card appears
  onMount(() => playRoundAudio(round.audioBase64, round.word.lemma));

  let answered = false;

  function answer(quality?: number) {
    if (answered) return;
    answered = true;
    sfx.pop();
    onAnswered(true, null, quality);
  }

  function onKey(e: KeyboardEvent) {
    if (!activates(e)) return;
    e.preventDefault();
    answer();
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- the `short:` spacing makes up for the second button's height: in the
  smallest window the card still ends above the bottom edge -->
<div class="card anim-pop mx-auto max-w-xl p-8 text-center short:p-6">
  <p class="text-lg font-bold uppercase tracking-widest text-tubo-500">✨ Palabra nueva</p>
  <h1 class="anim-pop my-5 text-6xl font-black tracking-tight short:my-3">{round.word.lemma}</h1>
  {#if round.word.pos}
    <p class="mb-2 text-sm font-semibold uppercase text-white/40">{posLabel(round.word.pos)} · {round.word.level}</p>
  {/if}
  <p class="text-2xl font-bold text-grape-400">{gloss}</p>

  {#if round.exampleEs}
    <div class="mt-6 rounded-2xl bg-white/5 p-4">
      <div class="flex items-center justify-center gap-2">
        <button
          class="pressable rounded-full bg-white/10 px-3 py-1 text-sm font-bold hover:bg-white/20"
          aria-label="Escuchar ejemplo"
          onclick={() => playRoundAudio(null, round.exampleEs ?? undefined)}
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
    class="pressable mt-8 w-full rounded-3xl bg-gradient-to-r from-lime-500 to-emerald-500 py-4 text-2xl font-extrabold text-night-900 shadow-xl shadow-lime-500/30 hover:brightness-110 short:mt-5"
    onclick={() => answer()}
  >
    ¡Lo tengo! 💪 <span class="ml-1 text-base font-bold text-night-900/50" aria-hidden="true">Enter</span>
  </button>
  <!-- No key of its own: Enter and Space are the main button's, and a word
    is not marked as known by a slip of the hand. -->
  <button
    class="pressable mt-2.5 rounded-2xl bg-white/5 px-5 py-1.5 text-base font-bold text-white/60 hover:bg-white/15 hover:text-white"
    onclick={() => answer(KNOWN_ALREADY)}
  >
    Ya me la sé
  </button>
</div>
