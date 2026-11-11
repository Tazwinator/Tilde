<script lang="ts">
  import { onDestroy } from "svelte";
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { confettiCenter } from "$lib/confetti";
  import { reportError } from "$lib/errors.svelte";
  import { plural } from "$lib/format";
  import { choiceFromKey } from "$lib/keys";
  import { answerFor, DONT_KNOW, DONT_KNOW_CHOICE, DONT_KNOW_KEY, startFromScratch } from "$lib/placement";
  import { sfx } from "$lib/sfx";
  import type { PlacementAnswer, PlacementItem, PlacementResult } from "$lib/contract";

  // how long an answered question shows which option was right
  const REVEAL_MS = 750;

  let items = $state<PlacementItem[]>([]);
  let idx = $state(0);
  // the option picked, or DONT_KNOW; null while the question is open
  let chosen = $state<number | null>(null);
  // never shown, only sent: plain data
  let answers: PlacementAnswer[] = [];
  let result = $state<PlacementResult | null>(null);
  let phase = $state<"intro" | "quiz" | "done">("intro");
  // one of the intro's two buttons has been pressed and its call is out
  let starting = $state(false);

  $effect(() => {
    // If placement was already done, send the user home. Not one that
    // "Empiezo de cero" is saving this moment: that one shows its result.
    void api.placementStatus().then((r) => {
      if (r !== null && phase === "intro" && items.length === 0 && !starting) goto("/");
    });
  });

  let revealTimer: ReturnType<typeof setTimeout> | undefined;
  let left = false;

  onDestroy(() => {
    left = true;
    clearTimeout(revealTimer);
  });

  async function begin() {
    if (starting) return;
    starting = true;
    sfx.click();
    try {
      items = await api.placementStart();
      idx = 0;
      answers = [];
      phase = "quiz";
    } finally {
      starting = false;
    }
  }

  // "Empiezo de cero" (see startFromScratch). No confetti: nothing has been
  // won yet.
  async function skip() {
    if (starting) return;
    starting = true;
    sfx.click();
    try {
      const res = await startFromScratch(api);
      if (left) return;
      result = res;
      phase = "done";
    } finally {
      starting = false;
    }
  }

  function pick(choice: number) {
    if (chosen !== null) return;
    chosen = choice;
    const answer = answerFor(items[idx], choice);
    // saying so is not a mistake, and doesn't sound like one
    if (choice === DONT_KNOW) sfx.click();
    else if (answer.correct) sfx.correct(0);
    else sfx.wrong();
    const given = [...answers, answer];
    const reveal = new Promise<void>((resolve) => {
      revealTimer = setTimeout(resolve, REVEAL_MS);
    });
    if (idx + 1 < items.length) {
      answers = given;
      void reveal.then(() => {
        chosen = null;
        idx += 1;
      });
      return;
    }
    // The last answer goes out at once, alongside the reveal: the result is
    // saved even if the player leaves before the reveal is over.
    void Promise.all([api.placementSubmit(given), reveal]).then(
      ([res]) => {
        if (left) return;
        answers = given;
        result = res;
        phase = "done";
        confettiCenter(120);
        sfx.fanfare();
      },
      (e) => {
        // said even after leaving (the error toast outlives the page): the
        // player must know that the placement was not saved
        reportError(e);
        if (left) return;
        // back to the last question, unanswered, so it can be given again
        chosen = null;
      },
    );
  }

  function onKey(e: KeyboardEvent) {
    if (phase !== "quiz" || chosen !== null || !items[idx]) return;
    const choice = choiceFromKey(e, items[idx].options.length, DONT_KNOW_CHOICE);
    if (choice !== null) pick(choice);
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="mx-auto flex min-h-[70vh] max-w-2xl flex-col justify-center gap-8 py-6">
  {#if phase === "intro"}
    <div class="card anim-pop p-8 text-center">
      <div class="anim-float mb-4 text-7xl" aria-hidden="true">🎯</div>
      <h1 class="text-4xl font-extrabold">Vamos a ver qué ya sabes</h1>
      <p class="mx-auto mt-3 max-w-md text-lg text-white/70">
        Unas preguntitas rápidas para calibrar tu punto de partida. Si no sabes una, pulsa «No lo sé»: es mejor que adivinar.
      </p>
      <button
        class="pressable mt-8 rounded-3xl bg-gradient-to-r from-grape-500 to-fuchsia-500 px-10 py-4 text-2xl font-extrabold shadow-xl shadow-grape-500/40 hover:brightness-110"
        disabled={starting}
        onclick={begin}
      >
        ¡Vamos! 🎲
      </button>
      <p class="mt-6 text-base text-white/50">¿Nunca has estudiado español?</p>
      <button
        class="pressable mt-2 rounded-2xl bg-white/10 px-6 py-2.5 text-lg font-bold hover:bg-white/20"
        disabled={starting}
        onclick={skip}
      >
        Empiezo de cero
      </button>
    </div>
  {:else if phase === "quiz" && items[idx]}
    {@const item = items[idx]}
    <div class="flex items-center justify-center gap-2" aria-label="Pregunta {idx + 1} de {items.length}">
      {#each items as _, i (i)}
        <span
          class="h-2.5 rounded-full transition-all duration-300
            {i < idx ? 'w-2.5 bg-lime-500' : i === idx ? 'w-7 bg-grape-400' : 'w-2.5 bg-white/15'}"
        ></span>
      {/each}
    </div>

    {#key idx}
      <div class="card anim-pop p-8 text-center">
        <p class="text-lg font-semibold text-white/50">¿Qué significa…?</p>
        <h1 class="anim-pop my-4 text-5xl font-black tracking-tight text-grape-400">{item.lemma}</h1>
        <div class="mt-6 grid gap-3 sm:grid-cols-2">
          {#each item.options as opt, oi (oi)}
            <button
              class="pressable rounded-2xl border-2 px-5 py-4 text-xl font-bold transition-all
                {chosen === null
                ? 'border-white/10 bg-white/5 hover:border-grape-400/60 hover:bg-white/10'
                : oi === item.answerIndex
                  ? 'border-lime-500 bg-lime-500/25'
                  : oi === chosen
                    ? 'border-coral-500 bg-coral-500/25'
                    : 'border-white/5 bg-white/[0.02] opacity-50'}"
              disabled={chosen !== null}
              onclick={() => pick(oi)}
            >
              <span class="mr-2 text-sm font-black text-white/40" aria-hidden="true">{oi + 1}</span>{opt}
            </button>
          {/each}
        </div>
        <!-- not one of the options: it sits apart from them, with a key of its own -->
        <button
          class="pressable mt-4 rounded-2xl px-6 py-2.5 text-lg font-bold transition-all
            {chosen === null
            ? 'bg-white/5 text-white/60 hover:bg-white/10 hover:text-white'
            : chosen === DONT_KNOW
              ? 'bg-white/15 text-white'
              : 'opacity-40'}"
          disabled={chosen !== null}
          onclick={() => pick(DONT_KNOW)}
        >
          <span class="mr-2 text-sm font-black text-white/40" aria-hidden="true">{DONT_KNOW_KEY}</span>No lo sé
        </button>
      </div>
    {/key}
  {:else if phase === "done" && result}
    <div class="card anim-pop p-8 text-center">
      <div class="anim-sparkle mb-4 text-7xl" aria-hidden="true">🎉</div>
      <h1 class="text-4xl font-extrabold">¡Listo para empezar!</h1>
      <p class="mt-3 text-xl text-white/75">
        Tu nivel de salida: <strong class="text-tubo-500">{result.cefrEstimate}</strong> ·
        {#if result.wordsMarkedKnown === 0}
          empezamos por las palabras más comunes.
        {:else}
          marcamos <strong class="text-lime-500">{plural(result.wordsMarkedKnown, "palabra", "palabras")}</strong>
          como {result.wordsMarkedKnown === 1 ? "conocida" : "conocidas"}.
        {/if}
      </p>
      <p class="mt-2 text-lg text-white/55">A partir de aquí todo lo que aprendas se queda contigo para siempre.</p>
      <button
        class="pressable mt-8 rounded-3xl bg-gradient-to-r from-lime-500 to-emerald-500 px-10 py-4 text-2xl font-extrabold text-night-900 shadow-xl shadow-lime-500/40 hover:brightness-110"
        onclick={() => {
          sfx.click();
          goto("/");
        }}
      >
        ¡Empezamos! →
      </button>
    </div>
  {/if}
</div>
