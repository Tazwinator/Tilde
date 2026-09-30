<script lang="ts">
  import type { Round } from "$lib/contract";
  import { norm } from "$lib/grading";
  import { playRoundAudio } from "$lib/audio";
  import { sfx } from "$lib/sfx";

  type RoundT = Extract<Round, { type: "listen_type" }>;

  let {
    round,
    onAnswered,
  }: {
    round: RoundT;
    onAnswered: (correct: boolean, correctText?: string | null) => void;
  } = $props();

  let typed = $state("");
  let checked = $state<null | boolean>(null);

  const lenientOk = $derived(checked !== null && checked);

  function play() {
    playRoundAudio(round.audioBase64, round.sentenceEs);
  }

  function submit(e: Event) {
    e.preventDefault();
    if (checked !== null || typed.trim() === "") return;
    const ok = norm(typed) === norm(round.answer);
    checked = ok;
    if (ok) sfx.correct(0);
    else sfx.wrong();
    setTimeout(() => onAnswered(ok, round.answer), 900);
  }
</script>

<div class="mx-auto max-w-xl">
  <h1 class="mb-6 text-center text-3xl font-extrabold">Escucha y escribe ✍️</h1>
  <div class="mb-6 flex justify-center">
    <button
      class="pressable flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-tubo-500 to-grape-500 text-3xl shadow-xl shadow-tubo-500/30 hover:brightness-110"
      aria-label="Reproducir audio"
      onclick={play}
    >
      🔊
    </button>
  </div>
  <form onsubmit={submit}>
    <input
      type="text"
      bind:value={typed}
      disabled={checked !== null}
      class="w-full rounded-3xl border-2 px-6 py-5 text-center text-2xl font-bold outline-none transition-colors
        {checked === null
        ? 'border-white/15 bg-white/5 focus:border-grape-400'
        : lenientOk
          ? 'border-lime-500 bg-lime-500/15'
          : 'anim-shake border-coral-500 bg-coral-500/15'}"
      placeholder="Escribe lo que oyes…"
      aria-label="Tu respuesta"
    />
    {#if checked === false}
      <p class="mt-3 text-center text-lg text-white/70">Casi — era: <strong class="text-lime-500">{round.answer}</strong></p>
    {/if}
    {#if checked === null}
      <button
        type="submit"
        class="pressable mt-5 w-full rounded-3xl bg-gradient-to-r from-grape-500 to-fuchsia-500 py-3.5 text-xl font-extrabold shadow-lg shadow-grape-500/30 hover:brightness-110 disabled:opacity-40"
        disabled={typed.trim() === ""}
      >
        Comprobar (Enter)
      </button>
    {/if}
  </form>
</div>
