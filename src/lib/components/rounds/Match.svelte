<script lang="ts">
  import type { Round } from "$lib/contract";
  import { textSize } from "$lib/defs";
  import { sfx } from "$lib/sfx";

  type RoundT = Extract<Round, { type: "match" }>;

  let {
    round,
    onAnswered,
  }: {
    round: RoundT;
    onAnswered: (correct: boolean, correctText?: string | null, quality?: number, missedWordIds?: number[]) => void;
  } = $props();

  interface Tile {
    wordId: number;
    text: string;
  }

  function shuffle<T>(a: T[]): T[] {
    const arr = [...a];
    for (let i = arr.length - 1; i > 0; i--) {
      const j = (Math.random() * (i + 1)) | 0;
      [arr[i], arr[j]] = [arr[j], arr[i]];
    }
    return arr;
  }

  const left: Tile[] = $derived(shuffle(round.pairs.map((p) => ({ wordId: p.wordId, text: p.es }))));
  const right: Tile[] = $derived(shuffle(round.pairs.map((p) => ({ wordId: p.wordId, text: p.en }))));

  let selectedEs = $state<number | null>(null);
  let matched = $state<number[]>([]);
  let wrongShake = $state<number | null>(null);
  let wrongAttempts = $state(0);
  // Spanish words the player tried to pair wrongly; only these are graded as missed.
  let missed = new Set<number>();
  let done = $state(false);

  function clickEs(t: Tile) {
    if (done || matched.includes(t.wordId)) return;
    selectedEs = t.wordId;
    sfx.pop();
  }

  function clickEn(t: Tile) {
    if (done || matched.includes(t.wordId) || selectedEs === null) return;
    if (selectedEs === t.wordId) {
      matched = [...matched, t.wordId];
      selectedEs = null;
      sfx.correct(matched.length);
      if (matched.length === round.pairs.length) {
        done = true;
        setTimeout(() => onAnswered(wrongAttempts <= 1, null, undefined, [...missed]), 600);
      }
    } else {
      wrongAttempts += 1;
      missed.add(selectedEs);
      wrongShake = t.wordId;
      sfx.wrong();
      setTimeout(() => (wrongShake = null), 450);
      selectedEs = null;
    }
  }
</script>

<div class="mx-auto max-w-2xl">
  <h1 class="mb-6 text-center text-3xl font-extrabold">Empareja cada palabra</h1>
  <div class="grid grid-cols-2 gap-x-5 gap-y-3">
    <div class="flex flex-col gap-3">
      {#each left as t (t.wordId)}
        <button
          class="min-h-16 rounded-2xl border-2 px-4 py-3 {textSize(t.text, 'text-xl')} font-bold transition-all
            {matched.includes(t.wordId)
            ? 'border-lime-500/60 bg-lime-500/20 text-lime-500'
            : selectedEs === t.wordId
              ? 'border-grape-400 bg-grape-500/30'
              : 'border-white/10 bg-white/5 hover:bg-white/10'}"
          disabled={matched.includes(t.wordId)}
          onclick={() => clickEs(t)}
        >
          {t.text}
        </button>
      {/each}
    </div>
    <div class="flex flex-col gap-3">
      {#each right as t (t.wordId)}
        <button
          class="min-h-16 rounded-2xl border-2 px-4 py-3 {textSize(t.text, 'text-xl')} font-bold transition-all
            {matched.includes(t.wordId)
            ? 'border-lime-500/60 bg-lime-500/20 text-lime-500'
            : wrongShake === t.wordId
              ? 'anim-shake border-coral-500 bg-coral-500/25'
              : 'border-white/10 bg-white/5 hover:bg-white/10'}"
          disabled={matched.includes(t.wordId)}
          onclick={() => clickEn(t)}
        >
          {t.text}
        </button>
      {/each}
    </div>
  </div>
  {#if selectedEs !== null}
    <p class="mt-4 text-center text-lg text-white/60">Ahora toca su traducción →</p>
  {/if}
</div>
