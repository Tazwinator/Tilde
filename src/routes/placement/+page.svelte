<script lang="ts">
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { confettiCenter } from "$lib/confetti";
  import { sfx } from "$lib/sfx";
  import type { PlacementAnswer, PlacementItem, PlacementResult } from "$lib/contract";

  const CHIPS = [
    { id: "ayer", label: "Empecé ayer", emoji: "🌱" },
    { id: "instituto", label: "Un poco de instituto", emoji: "🏫" },
    { id: "intermedio", label: "Intermedio", emoji: "🚀" },
  ];

  let items = $state<PlacementItem[]>([]);
  let idx = $state(0);
  let chosen = $state<number | null>(null);
  let startChip = $state<string | null>(null);
  let answers = $state<PlacementAnswer[]>([]);
  let result = $state<PlacementResult | null>(null);
  let phase = $state<"intro" | "quiz" | "done">("intro");

  $effect(() => {
    // If placement was already done, send the user home.
    void api.placementStatus().then((r) => {
      if (r !== null && phase === "intro" && items.length === 0) goto("/");
    });
  });

  async function begin() {
    sfx.click();
    items = await api.placementStart();
    idx = 0;
    answers = [];
    phase = "quiz";
  }

  function pick(optionIdx: number) {
    if (chosen !== null) return;
    chosen = optionIdx;
    const item = items[idx];
    const correct = optionIdx === item.answerIndex;
    answers.push({ wordId: item.wordId, correct });
    if (correct) sfx.correct(0);
    else sfx.wrong();
    setTimeout(async () => {
      chosen = null;
      if (idx + 1 >= items.length) {
        const res = await api.placementSubmit(answers, startChip ?? undefined);
        result = res;
        phase = "done";
        confettiCenter(120);
        sfx.fanfare();
      } else {
        idx += 1;
      }
    }, 750);
  }

  function onKey(e: KeyboardEvent) {
    if (phase === "quiz" && chosen === null) {
      const n = parseInt(e.key, 10);
      if (n >= 1 && n <= 4) pick(n - 1);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="mx-auto flex min-h-[70vh] max-w-2xl flex-col justify-center gap-8 py-6">
  {#if phase === "intro"}
    <div class="card anim-pop p-8 text-center">
      <div class="anim-float mb-4 text-7xl" aria-hidden="true">🎯</div>
      <h1 class="text-4xl font-extrabold">Vamos a ver qué ya sabes</h1>
      <p class="mx-auto mt-3 max-w-md text-lg text-white/70">
        12 preguntitas rápidas. Si no sabes una, ¡no pasa nada! Solo es para calibrar tu punto de partida.
      </p>
      <h2 class="mt-8 text-lg font-bold text-white/80">¿Cómo llevas el español?</h2>
      <div class="mt-3 flex flex-wrap justify-center gap-3">
        {#each CHIPS as c (c.id)}
          <button
            class="pressable rounded-full border px-5 py-2.5 text-lg font-bold transition-colors
              {startChip === c.id
              ? 'border-grape-400 bg-grape-500/30 text-white'
              : 'border-white/15 bg-white/5 text-white/70 hover:bg-white/10'}"
            class:ring-2={startChip === c.id}
            class:ring-grape-400={startChip === c.id}
            onclick={() => (startChip = startChip === c.id ? null : c.id)}
          >
            <span aria-hidden="true">{c.emoji}</span> {c.label}
          </button>
        {/each}
      </div>
      <button
        class="pressable mt-8 rounded-3xl bg-gradient-to-r from-grape-500 to-fuchsia-500 px-10 py-4 text-2xl font-extrabold shadow-xl shadow-grape-500/40 hover:brightness-110"
        onclick={begin}
      >
        ¡Vamos! 🎲
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
      </div>
    {/key}
  {:else if phase === "done" && result}
    <div class="card anim-pop p-8 text-center">
      <div class="anim-sparkle mb-4 text-7xl" aria-hidden="true">🎉</div>
      <h1 class="text-4xl font-extrabold">¡Listo para empezar!</h1>
      <p class="mt-3 text-xl text-white/75">
        Tu nivel de salida: <strong class="text-tubo-500">{result.cefrEstimate}</strong> ·
        marcamos <strong class="text-lime-500">{result.wordsMarkedKnown} palabras</strong> como conocidas.
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
