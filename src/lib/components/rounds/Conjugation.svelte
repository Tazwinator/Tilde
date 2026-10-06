<script lang="ts">
  import type { Round } from "$lib/contract";
  import { grade } from "$lib/grading";
  import { typingInto } from "$lib/keys";
  import { sfx } from "$lib/sfx";

  type RoundT = Extract<Round, { type: "conjugation" }>;

  let {
    round,
    onAnswered,
  }: {
    round: RoundT;
    onAnswered: (correct: boolean, correctText?: string | null) => void;
  } = $props();

  let typed = $state("");
  let checked = $state<null | boolean>(null);
  let accentSlip = $state(false);

  function submit(e: Event) {
    e.preventDefault();
    if (checked !== null || typed.trim() === "") return;
    // the accent is part of the form here: "hablo" (I speak) is not "habló" (he spoke)
    const verdict = grade(typed, round.answer);
    const ok = verdict === "right";
    accentSlip = verdict === "accents";
    checked = ok;
    if (ok) sfx.correct(0);
    else sfx.wrong();
    setTimeout(() => onAnswered(ok, round.answer), 900);
  }

  function onKey(e: KeyboardEvent) {
    // inside the input, the form submits on Enter by itself
    if (e.key === "Enter" && !typingInto(e)) {
      const form = document.getElementById("conj-form") as HTMLFormElement | null;
      form?.requestSubmit();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="mx-auto max-w-xl">
  <h1 class="mb-6 text-center text-3xl font-extrabold">Conjuga el verbo 🔮</h1>
  <div class="card mb-6 p-6 text-center">
    <p class="text-3xl font-black">
      <span class="text-grape-400">{round.person}</span> ___
      <span class="text-xl font-semibold text-white/50">({round.verb} · {round.tense})</span>
    </p>
  </div>

  <div class="mb-5 flex justify-center">
    <span class="rounded-full bg-mango-500/15 px-5 py-2 text-2xl font-bold tracking-widest text-mango-500" title="Pista">
      {round.hint}
    </span>
  </div>

  <form id="conj-form" onsubmit={submit}>
    <input
      type="text"
      bind:value={typed}
      disabled={checked !== null}
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      class="w-full rounded-3xl border-2 px-6 py-5 text-center text-2xl font-bold outline-none transition-colors
        {checked === null
        ? 'border-white/15 bg-white/5 focus:border-grape-400'
        : checked
          ? 'border-lime-500 bg-lime-500/15'
          : 'anim-shake border-coral-500 bg-coral-500/15'}"
      placeholder="Escribe la forma…"
      aria-label="Forma del verbo"
    />
    {#if checked === false && accentSlip}
      <p class="mt-3 text-center text-lg text-white/70">¡Casi! Solo falta la tilde: <strong class="text-lime-500">{round.answer}</strong></p>
    {:else if checked === false}
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
