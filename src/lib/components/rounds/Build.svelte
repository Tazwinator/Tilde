<script lang="ts">
  import type { Round } from "$lib/contract";
  import { norm } from "$lib/grading";
  import { sfx } from "$lib/sfx";

  type RoundT = Extract<Round, { type: "build" }>;

  let {
    round,
    onAnswered,
  }: {
    round: RoundT;
    onAnswered: (correct: boolean, correctText?: string | null) => void;
  } = $props();

  interface Tile {
    id: number;
    word: string;
  }

  function shuffle<T>(a: T[]): T[] {
    const arr = [...a];
    for (let i = arr.length - 1; i > 0; i--) {
      const j = (Math.random() * (i + 1)) | 0;
      [arr[i], arr[j]] = [arr[j], arr[i]];
    }
    return arr;
  }

  // svelte-ignore state_referenced_locally -- `round` is fixed for this component's lifetime (keyed by round index)
  const pool: Tile[] = $state(round.tiles.map((word, id) => ({ id, word })));
  let row = $state<number[]>([]);
  let checked = $state<null | boolean>(null);

  const answerText = $derived(norm(round.answer));

  function place(t: Tile) {
    if (checked !== null) return;
    pool.splice(
      pool.findIndex((x) => x.id === t.id),
      1,
    );
    row = [...row, t.id];
    sfx.pop();
  }

  function remove(tileId: number) {
    if (checked !== null) return;
    row = row.filter((id) => id !== tileId);
    pool.push({ id: tileId, word: round.tiles[tileId] });
    sfx.click();
  }

  function check() {
    if (checked !== null || row.length === 0) return;
    const attempt = row.map((id) => round.tiles[id]).join(" ");
    const ok = norm(attempt) === answerText;
    checked = ok;
    if (ok) sfx.correct(0);
    else sfx.wrong();
    setTimeout(() => onAnswered(ok, round.answer), 900);
  }

  function onKey(e: KeyboardEvent) {
    // Enter on a focused tile places or removes that tile (its own click);
    // only an Enter with no button focused means "Comprobar"
    if (e.key !== "Enter" || e.target instanceof HTMLButtonElement) return;
    check();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="mx-auto max-w-2xl">
  <h1 class="mb-2 text-center text-3xl font-extrabold">Construye la frase 🧩</h1>
  <p class="mb-6 text-center text-lg text-white/55">💡 {round.sentenceEn}</p>

  <div
    class="mb-5 flex min-h-20 flex-wrap content-start items-start gap-2.5 rounded-3xl border-2 border-dashed p-4 transition-colors
      {checked === null ? 'border-white/15 bg-white/[0.03]' : checked ? 'border-lime-500/70 bg-lime-500/10' : 'anim-shake border-coral-500/70 bg-coral-500/10'}"
    aria-label="Tu frase"
  >
    {#if row.length === 0}
      <span class="text-lg text-white/30">Toca las palabras de abajo…</span>
    {/if}
    {#each row as tileId (tileId)}
      <button
        class="anim-pop rounded-2xl bg-grape-500/40 px-4 py-2.5 text-xl font-bold ring-1 ring-grape-400/50 hover:bg-coral-500/40"
        title="Quitar palabra"
        disabled={checked !== null}
        onclick={() => remove(tileId)}
      >
        {round.tiles[tileId]}
      </button>
    {/each}
  </div>

  <div class="flex flex-wrap gap-2.5">
    {#each pool as t (t.id)}
      <button
        class="rounded-2xl border border-white/10 bg-white/8 px-4 py-2.5 text-xl font-bold hover:bg-white/15"
        disabled={checked !== null}
        onclick={() => place(t)}
      >
        {t.word}
      </button>
    {/each}
  </div>

  {#if checked === null}
    <button
      class="pressable mt-6 w-full rounded-3xl bg-gradient-to-r from-grape-500 to-fuchsia-500 py-3.5 text-xl font-extrabold shadow-lg shadow-grape-500/30 hover:brightness-110 disabled:opacity-40"
      disabled={row.length === 0}
      onclick={check}
    >
      Comprobar
    </button>
  {:else if !checked}
    <p class="mt-4 text-center text-lg text-white/70">Casi — era: <strong class="text-lime-500">{round.answer}</strong></p>
  {/if}
</div>
