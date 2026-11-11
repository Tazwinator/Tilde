<script lang="ts">
  import { tick } from "svelte";
  import { prefersReducedMotion } from "svelte/motion";
  import { api } from "$lib/api";
  import { confettiCenter } from "$lib/confetti";
  import { sfx } from "$lib/sfx";
  import type { ConjRow, WordDetail, WordHit } from "$lib/contract";
  import { glossText, posLabel } from "$lib/defs";
  import { dueIn } from "$lib/format";
  import { createWordCard } from "$lib/wordCard";

  let query = $state("");
  let results = $state<WordHit[]>([]);
  let detail = $state<WordDetail | null>(null);
  let defLang = $state("en");
  let searching = $state(false);
  let activeTense = $state<string | null>(null);
  let detailEl = $state<HTMLElement>();
  let detailHeading = $state<HTMLElement>();

  const tenses = $derived.by(() => {
    const rows: ConjRow[] = detail?.conjugations ?? [];
    return [...new Set(rows.map((r) => r.tense))];
  });
  // the table opens on the present, wherever the data lists it
  const shownTense = $derived(activeTense ?? (tenses.includes("presente") ? "presente" : tenses[0]));
  const tenseRows = $derived((detail?.conjugations ?? []).filter((r) => r.tense === shownTense));

  // Both lines are joined here, not in the markup: there Svelte trims the
  // space at the edge of a block, and "verbo ·puesto" is what came out.
  const meta = $derived(
    detail
      ? [detail.word.pos && posLabel(detail.word.pos), `puesto n.º ${detail.word.rank}`, detail.word.level].filter(Boolean).join(" · ")
      : "",
  );
  // "conocida" or "aprendiendo", by the same flag as the list beside it and
  // Home's count: never the scheduler's own state, which says something else
  const cardLine = $derived(
    detail?.card
      ? [
          detail.word.known ? "conocida" : "aprendiendo",
          `repaso ${dueIn(detail.card.dueInDays)}`,
          `fuerza ${Math.round(detail.card.strength * 100)}%`,
        ].join(" · ")
      : "",
  );

  let debounce: ReturnType<typeof setTimeout> | undefined;
  function onInput() {
    clearTimeout(debounce);
    debounce = setTimeout(() => void doSearch(), 250);
  }

  // Answers can come back out of order; only the latest search fills the list.
  let searches = 0;
  async function doSearch() {
    const mine = ++searches;
    searching = true;
    try {
      const found = await api.searchWords(query.trim(), 50);
      if (mine === searches) results = found;
    } finally {
      if (mine === searches) searching = false;
    }
  }

  // The same for a word's card, which is asked for by opening a word and by
  // the two buttons on the open one: the last one asked for is the one shown.
  const card = createWordCard(api);

  async function open(w: WordHit) {
    sfx.click();
    const found = await card.open(w.wordId);
    if (!found) return;
    detail = found;
    activeTense = null;
    await tick();
    if (!detailEl) return;
    // In one column the card opens below the list, out of sight. Beside the
    // list its top is already showing, and then the page stays where it is:
    // "nearest" would not do, as it pulls any card taller than the window up
    // to the top. (Measured on the panel, not the card, which is mid-pop.)
    const top = detailEl.getBoundingClientRect().top;
    if (top < 0 || top > window.innerHeight / 2) {
      detailEl.scrollIntoView({ block: "start", behavior: prefersReducedMotion.current ? "auto" : "smooth" });
    }
    // and focus goes with it, so the card's buttons are the next Tab stop
    detailHeading?.focus({ preventScroll: true });
  }

  async function markKnown() {
    if (!detail) return;
    const found = await card.markKnown(detail.word.wordId);
    // only now that it is saved: a change that failed must not sound like one that worked
    sfx.correct(0);
    confettiCenter(60);
    if (found) detail = found;
    await doSearch();
  }

  async function resetWord() {
    if (!detail) return;
    sfx.click();
    const found = await card.reset(detail.word.wordId);
    if (found) detail = found;
    await doSearch();
  }

  $effect(() => {
    void (async () => {
      defLang = (await api.profile()).definitionLang;
      await doSearch();
    })();
  });
</script>

<div class="flex flex-col gap-6">
  <header>
    <h1 class="text-4xl font-extrabold">Explorar 🔍</h1>
    <p class="mt-1 text-lg text-white/60">Busca cualquier palabra y mira todo lo que sabemos de ella.</p>
  </header>

  <input
    type="search"
    bind:value={query}
    oninput={onInput}
    class="w-full rounded-3xl border-2 border-white/15 bg-white/5 px-6 py-4 text-xl font-semibold outline-none transition-colors focus:border-grape-400"
    placeholder="Buscar palabra o forma… (p. ej. hablar, tengo)"
    aria-label="Buscar palabras"
  />

  <div class="grid gap-6 lg:grid-cols-[1fr_1.2fr]">
    <!-- Results list -->
    <div class="flex max-h-[60vh] flex-col gap-2 overflow-y-auto pr-1" aria-label="Resultados">
      {#if searching && results.length === 0}
        <p class="p-4 text-lg text-white/50">Buscando…</p>
      {:else if results.length === 0}
        <div class="card p-8 text-center">
          <p class="text-4xl" aria-hidden="true">🕳️</p>
          <p class="mt-2 text-lg text-white/60">Nada por aquí. ¡Prueba otra palabra!</p>
        </div>
      {/if}
      {#each results as w (w.wordId)}
        <button
          class="card card-hover pressable flex items-center gap-3 p-4 text-left"
          onclick={() => open(w)}
        >
          <span class="text-2xl font-extrabold">{w.lemma}</span>
          <span class="min-w-0 flex-1 truncate text-base text-white/55">{glossText(w, defLang)}</span>
          <span class="rounded-full bg-tubo-500/15 px-2.5 py-0.5 text-sm font-bold text-tubo-500">{w.level}</span>
          <span
            class="whitespace-nowrap rounded-full px-2.5 py-0.5 text-sm font-bold
              {w.known ? 'bg-lime-500/20 text-lime-500' : w.inDeck ? 'bg-tubo-500/15 text-tubo-500' : 'bg-white/10 text-white/50'}"
          >
            {w.known ? "✓ conocida" : w.inDeck ? "aprendiendo" : "nueva"}
          </span>
        </button>
      {/each}
    </div>

    <!-- Detail panel -->
    <div bind:this={detailEl} class="scroll-mt-6">
      {#if detail}
        <div class="card anim-pop p-6">
          <div class="flex items-start gap-3">
            <div class="min-w-0 flex-1">
              <h2 bind:this={detailHeading} tabindex="-1" class="text-4xl font-black">{detail.word.lemma}</h2>
              <p class="mt-0.5 text-base text-white/50">{meta}</p>
            </div>
            <button
              class="pressable flex h-12 w-12 items-center justify-center rounded-full bg-gradient-to-br from-tubo-500 to-grape-500 text-xl shadow-lg hover:brightness-110"
              aria-label="Pronunciar"
              onclick={() => void api.speak(detail!.word.lemma)}
            >
              🔊
            </button>
          </div>

          <dl class="mt-4 grid gap-2 text-lg">
            <div class="flex gap-2">
              <dt class="w-10 shrink-0 text-white/40">EN</dt>
              <dd class="font-semibold">{detail.word.glossEn ?? "—"}</dd>
            </div>
            <div class="flex gap-2">
              <dt class="w-10 shrink-0 text-white/40">ES</dt>
              <dd class="font-semibold">{detail.word.glossEs ?? "—"}</dd>
            </div>
          </dl>

          {#if detail.card}
            <p class="mt-3 inline-block rounded-full bg-white/10 px-4 py-1.5 text-base font-bold">📚 {cardLine}</p>
          {:else}
            <p class="mt-3 inline-block rounded-full bg-white/10 px-4 py-1.5 text-base font-bold">🌱 sin tarjeta todavía</p>
          {/if}

          {#if detail.sentences.length > 0}
            <h3 class="mt-5 text-lg font-extrabold">Ejemplos</h3>
            {#each detail.sentences as s (s.id)}
              <div class="mt-2 rounded-2xl bg-white/5 p-3">
                <p class="text-lg italic">"{s.es}"</p>
                {#if s.en}<p class="text-base text-white/55">{s.en}</p>{/if}
              </div>
            {/each}
          {/if}

          {#if detail.conjugations && detail.conjugations.length > 0}
            <h3 class="mt-5 text-lg font-extrabold">Conjugación</h3>
            <div class="mt-2 flex flex-wrap gap-2">
              {#each tenses as t (t)}
                <button
                  class="rounded-full px-4 py-1.5 text-base font-bold transition-colors
                    {shownTense === t ? 'bg-grape-500 text-white' : 'bg-white/10 text-white/60 hover:bg-white/20'}"
                  onclick={() => (activeTense = t)}
                >
                  {t}
                </button>
              {/each}
            </div>
            <table class="mt-3 w-full text-lg">
              <tbody>
                {#each tenseRows as r (r.person + r.tense)}
                  <tr class="border-b border-white/5 last:border-0">
                    <td class="py-1.5 pr-4 text-white/55">{r.person}</td>
                    <td class="py-1.5 font-bold">{r.form}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}

          <div class="mt-6 flex gap-3">
            <button
              class="pressable flex-1 rounded-2xl bg-lime-500 py-3 text-lg font-extrabold text-night-900 hover:brightness-110 {detail.word.known ? 'opacity-40' : ''}"
              disabled={detail.word.known}
              onclick={markKnown}
            >
              ✓ Marcar como conocida
            </button>
            <button class="pressable rounded-2xl bg-white/10 px-5 py-3 text-lg font-bold hover:bg-white/20" onclick={resetWord}>
              ↺ Reiniciar
            </button>
          </div>
        </div>
      {:else}
        <div class="card flex h-full min-h-60 flex-col items-center justify-center p-8 text-center">
          <span class="anim-float text-5xl" aria-hidden="true">📖</span>
          <p class="mt-3 text-lg text-white/50">Toca una palabra para ver su ficha completa.</p>
        </div>
      {/if}
    </div>
  </div>
</div>
