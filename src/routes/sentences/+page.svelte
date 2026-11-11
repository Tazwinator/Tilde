<script lang="ts">
  import { api } from "$lib/api";
  import { decodeText } from "$lib/decode";
  import { confettiCenter } from "$lib/confetti";
  import { plural } from "$lib/format";
  import { sfx } from "$lib/sfx";
  import type { ImportReport, MinedSentence } from "$lib/contract";

  // The collection comes a page at a time, newest first; `total` is all of it.
  let sentences = $state<MinedSentence[]>([]);
  let total = $state(0);
  let loadingMore = $state(false);
  let report = $state<ImportReport | null>(null);
  let importing = $state(false);
  let fileInput: HTMLInputElement | undefined = $state();

  // counts the times the list started over (an import does that), so that a
  // page asked for before then is not added to the new list
  let restarts = 0;

  async function loadFirst() {
    const list = ++restarts;
    const page = await api.listSentences();
    if (list !== restarts) return;
    sentences = page.sentences;
    total = page.total;
  }

  async function loadMore() {
    const last = sentences.at(-1);
    if (!last || loadingMore) return;
    const list = restarts;
    loadingMore = true;
    try {
      const page = await api.listSentences(last.id);
      if (list !== restarts) return;
      sentences = [...sentences, ...page.sentences];
      total = page.total;
    } finally {
      loadingMore = false;
    }
  }

  $effect(() => {
    void loadFirst();
  });

  async function onFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    importing = true;
    try {
      const text = decodeText(new Uint8Array(await file.arrayBuffer()));
      report = await api.importSrt(file.name, text);
      await loadFirst();
      // a file that added nothing is not a win: no confetti for it
      if (report.sentencesAdded > 0) {
        confettiCenter(130);
        sfx.fanfare();
      }
    } finally {
      importing = false;
      input.value = "";
    }
  }

  async function addToDeck(id: number) {
    await api.addSentenceToDeck(id);
    // the sound and the tick come once it is saved, never before
    sfx.pop();
    sentences = sentences.map((s) => (s.id === id ? { ...s, inDeck: true } : s));
  }
</script>

<div class="flex flex-col gap-6">
  <header>
    <h1 class="text-4xl font-extrabold">Frases 💬</h1>
    <p class="mt-1 text-lg text-white/60">
      Mina frases de subtítulos y textos reales. El español de verdad, el que se habla en la calle.
    </p>
  </header>

  <!-- Import zone -->
  <section class="card border-dashed p-8 text-center" aria-label="Importar subtítulos">
    <div class="anim-float text-5xl" aria-hidden="true">📥</div>
    <h2 class="mt-2 text-2xl font-extrabold">Importa un archivo</h2>
    <p class="mt-1 text-lg text-white/60">Subtítulos (.srt / .vtt) o texto (.txt) — lo leemos todo en local.</p>
    <input
      bind:this={fileInput}
      type="file"
      accept=".srt,.vtt,.txt"
      class="hidden"
      aria-label="Seleccionar archivo de subtítulos"
      onchange={onFile}
    />
    <button
      class="pressable mt-5 rounded-3xl bg-gradient-to-r from-tubo-500 to-grape-500 px-8 py-3.5 text-xl font-extrabold shadow-lg shadow-tubo-500/30 hover:brightness-110"
      disabled={importing}
      onclick={() => fileInput?.click()}
    >
      {importing ? "Procesando…" : "Elegir archivo 📄"}
    </button>

    {#if report}
      <div class="anim-pop mx-auto mt-6 max-w-lg rounded-3xl bg-white/5 p-5 text-left">
        {#if report.sentencesAdded > 0}
          <p class="text-xl font-extrabold text-lime-500">¡Importado: {report.fileTitle}!</p>
        {:else if report.sentencesFound === 0}
          <p class="text-xl font-extrabold text-white/70">No hemos encontrado frases en «{report.fileTitle}»</p>
          <p class="mt-1 text-base text-white/60">¿Es un archivo de subtítulos (.srt, .vtt) o de texto?</p>
        {:else}
          <p class="text-xl font-extrabold text-white/70">Nada nuevo en «{report.fileTitle}»</p>
          <p class="mt-1 text-base text-white/60">Ya tenías estas frases o no reconocimos suficientes palabras.</p>
        {/if}
        {#if report.sentencesFound > 0}
          <div class="mt-3 grid grid-cols-3 gap-3 text-center">
            <div class="rounded-2xl bg-white/5 p-3">
              <p class="text-2xl font-black">{report.sentencesFound}</p>
              <p class="text-sm text-white/50">frases vistas</p>
            </div>
            <div class="rounded-2xl bg-white/5 p-3">
              <p class="text-2xl font-black text-tubo-500">{report.sentencesAdded}</p>
              <p class="text-sm text-white/50">añadidas</p>
            </div>
            <div class="rounded-2xl bg-white/5 p-3">
              <p class="text-2xl font-black text-lime-500">{report.wordsMatched}</p>
              <p class="text-sm text-white/50">palabras reconocidas</p>
            </div>
          </div>
        {/if}
        {#if report.topWords.length > 0}
          <p class="mt-4 text-base font-bold text-white/60">Las palabras que más aparecen:</p>
          <div class="mt-2 flex flex-wrap gap-2">
            {#each report.topWords as w (w.wordId)}
              <span class="rounded-full bg-lime-500/15 px-3 py-1 text-base font-bold text-lime-500">{w.lemma}</span>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </section>

  <!-- Mined sentences -->
  <section aria-label="Frases minadas">
    <h2 class="mb-3 text-2xl font-extrabold">Tu colección <span class="text-lg font-semibold text-white/50">({total})</span></h2>
    {#if sentences.length === 0}
      <div class="card p-8 text-center">
        <p class="text-4xl" aria-hidden="true">💬</p>
        <p class="mt-2 text-lg text-white/60">Aún no hay frases. ¡Importa tus primeros subtítulos arriba!</p>
      </div>
    {/if}
    <div class="flex flex-col gap-3">
      {#each sentences as s (s.id)}
        <div class="card flex items-center gap-3 p-4">
          <button
            class="pressable flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-white/10 text-lg hover:bg-white/20"
            aria-label="Escuchar frase"
            onclick={() => void api.speak(s.es)}
          >
            🔊
          </button>
          <div class="min-w-0 flex-1">
            <p class="text-lg font-semibold leading-snug">{s.es}</p>
            <span class="mt-1 inline-block rounded-full bg-white/10 px-2.5 py-0.5 text-xs font-bold text-white/50">{s.source}</span>
          </div>
          {#if s.inDeck}
            <span class="rounded-full bg-lime-500/20 px-3.5 py-1.5 text-sm font-extrabold text-lime-500">✓ en el mazo</span>
          {:else}
            <button
              class="pressable rounded-full bg-grape-500 px-3.5 py-1.5 text-sm font-extrabold hover:brightness-110"
              onclick={() => addToDeck(s.id)}
            >
              + Añadir al mazo
            </button>
          {/if}
        </div>
      {/each}
    </div>
    {#if sentences.length < total}
      <button
        class="pressable mx-auto mt-4 block rounded-full bg-white/10 px-6 py-2.5 text-base font-bold hover:bg-white/20"
        disabled={loadingMore}
        onclick={loadMore}
      >
        Ver {plural(total - sentences.length, "frase más", "frases más")}
      </button>
    {/if}
  </section>
</div>
