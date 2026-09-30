<script lang="ts">
  import { api } from "$lib/api";
  import { confettiCenter } from "$lib/confetti";
  import { sfx } from "$lib/sfx";
  import type { ConjRow, WordDetail, WordHit } from "$lib/contract";
  import { glossText, posLabel } from "$lib/defs";

  let query = $state("");
  let results = $state<WordHit[]>([]);
  let detail = $state<WordDetail | null>(null);
  let defLang = $state("en");
  let searching = $state(false);
  let activeTense = $state<string | null>(null);

  const tenses = $derived.by(() => {
    const rows: ConjRow[] = detail?.conjugations ?? [];
    return [...new Set(rows.map((r) => r.tense))];
  });
  const tenseRows = $derived((detail?.conjugations ?? []).filter((r) => r.tense === (activeTense ?? tenses[0])));

  let debounce: ReturnType<typeof setTimeout> | undefined;
  function onInput() {
    clearTimeout(debounce);
    debounce = setTimeout(() => void doSearch(), 250);
  }

  async function doSearch() {
    searching = true;
    results = await api.searchWords(query.trim(), 50);
    searching = false;
  }

  async function open(w: WordHit) {
    sfx.click();
    detail = await api.wordDetail(w.wordId);
    activeTense = null;
  }

  async function markKnown() {
    if (!detail) return;
    sfx.correct(0);
    confettiCenter(60);
    await api.markWordKnown(detail.word.wordId);
    detail = await api.wordDetail(detail.word.wordId);
    await doSearch();
  }

  async function resetWord() {
    if (!detail) return;
    sfx.click();
    await api.resetWord(detail.word.wordId);
    detail = await api.wordDetail(detail.word.wordId);
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
    placeholder="Buscar palabra… (p. ej. hablar)"
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
            class="rounded-full px-2.5 py-0.5 text-sm font-bold {w.known ? 'bg-lime-500/20 text-lime-500' : 'bg-white/10 text-white/50'}"
          >
            {w.known ? "✓ conocida" : "nueva"}
          </span>
        </button>
      {/each}
    </div>

    <!-- Detail panel -->
    <div>
      {#if detail}
        <div class="card anim-pop p-6">
          <div class="flex items-start gap-3">
            <div class="min-w-0 flex-1">
              <h2 class="text-4xl font-black">{detail.word.lemma}</h2>
              <p class="mt-0.5 text-base text-white/50">
                {#if detail.word.pos}{posLabel(detail.word.pos)} · {/if}rank #{detail.word.rank} · {detail.word.level}
              </p>
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
            <p class="mt-3 inline-block rounded-full bg-white/10 px-4 py-1.5 text-base font-bold">
              📚 estado: {detail.card.state} · vence en {detail.card.dueInDays} día{detail.card.dueInDays === 1 ? "" : "s"} · fuerza {Math.round(detail.card.strength * 100)}%
            </p>
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
                    {(activeTense ?? tenses[0]) === t ? 'bg-grape-500 text-white' : 'bg-white/10 text-white/60 hover:bg-white/20'}"
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
