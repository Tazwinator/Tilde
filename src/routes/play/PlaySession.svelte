<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { beforeNavigate, goto } from "$app/navigation";
  import { page } from "$app/state";
  import { api } from "$lib/api";
  import { stopAudio } from "$lib/audio";
  import { badgeSound, celebrate } from "$lib/celebrate.svelte";
  import { confetti } from "$lib/confetti";
  import { pillShape } from "$lib/defs";
  import { reportError } from "$lib/errors.svelte";
  import { accuracyRing, sessionMinutes } from "$lib/format";
  import { needsPacking, watchOverflow, type Overflow } from "$lib/overflow";
  import { comboMilestone, createFlow, initialView, mergeBadges, PLAY_REVEAL_MS, type FlowView } from "$lib/sessionFlow";
  import { sfx } from "$lib/sfx";
  import type { RoundFeedback, SessionSummary } from "$lib/contract";
  import Modal from "$lib/components/Modal.svelte";
  import NewWord from "$lib/components/rounds/NewWord.svelte";
  import Choice from "$lib/components/rounds/Choice.svelte";
  import Match from "$lib/components/rounds/Match.svelte";
  import Listen from "$lib/components/rounds/Listen.svelte";
  import ListenType from "$lib/components/rounds/ListenType.svelte";
  import Build from "$lib/components/rounds/Build.svelte";
  import Cloze from "$lib/components/rounds/Cloze.svelte";
  import Conjugation from "$lib/components/rounds/Conjugation.svelte";
  import ReviewCard from "$lib/components/rounds/ReviewCard.svelte";

  // One session, from "Preparando la mesa…" to the summary. The play page
  // makes a new instance for every session (see +page.svelte), so nothing
  // here is ever reset by hand: a new session can't inherit an old one's
  // state. What happens when lives in sessionFlow.ts; this is the screen.
  let {
    requested,
    onAgain,
  }: {
    /** `?kind=` from the URL, unchecked */
    requested: string | null;
    /** start another session like this one */
    onAgain: () => void;
  } = $props();

  const PRAISE = ["¡Bien!", "¡Perfecto!", "¡Excelente!", "¡Genial!", "¡Muy bien!", "¡Brutal!"];

  // The flow writes to this from outside the component. It stays reactive
  // because it is one `$state` object, always read as `view.x`: never
  // destructure it.
  const view = $state<FlowView>(initialView());

  // svelte-ignore state_referenced_locally -- `requested` is fixed for this instance's lifetime (the page keys on it)
  const flow = createFlow(
    view,
    {
      api,
      clock: {
        setTimeout: (fn, ms) => setTimeout(fn, ms),
        clearTimeout: (id) => clearTimeout(id as ReturnType<typeof setTimeout>),
        now: () => performance.now(),
      },
      celebrate,
      reportError,
      flashText,
      onFeedback,
      onDone,
      revealMs: (round) => PLAY_REVEAL_MS[round.type],
    },
    { requested },
  );

  function flashText(fb: RoundFeedback, correct: boolean, correctText: string | null): string {
    if (!correct) return correctText ? `Casi — era «${correctText}»` : "Casi — ¡sigue así!";
    if (fb.levelUp) return `¡NIVEL ${fb.levelUp.level}! ${fb.levelUp.name}`;
    return PRAISE[(Math.random() * PRAISE.length) | 0];
  }

  function onFeedback(fb: RoundFeedback, comboBefore: number) {
    if (fb.levelUp) {
      sfx.levelUp();
      confetti(window.innerWidth / 2, window.innerHeight * 0.3, 140);
    } else if (comboMilestone(comboBefore, fb.combo)) {
      confetti(window.innerWidth / 2, window.innerHeight * 0.35, 90);
    }
  }

  function onDone(summary: SessionSummary) {
    if (summary.accuracy >= 0.8) {
      confetti(window.innerWidth / 2, window.innerHeight * 0.3, 160);
      sfx.fanfare();
    }
    // the summary lists them, so no toast: just the sound
    if (summary.newBadges.length > 0) badgeSound();
  }

  onMount(() => void flow.start());

  // Leaving goes quiet here, before the layout's page transition: onDestroy
  // only runs once the old page has faded out. A click on the link to the
  // page we are already on is not leaving.
  beforeNavigate(({ to }) => {
    if (to && to.url.pathname === page.url.pathname && to.url.search === page.url.search) return;
    flow.leave();
    stopAudio();
  });

  onDestroy(() => {
    flow.leave();
    stopAudio();
  });

  const round = $derived(view.session?.rounds[view.idx] ?? null);
  const badges = $derived(mergeBadges(view.roundBadges, view.summary?.newBadges ?? []));

  let showExit = $state(false);
  let roundBox = $state<HTMLDivElement>();

  // How the summary's card fits (see the summary). A summary that doesn't
  // fit as it is laid out packs itself tight, and only what is then still
  // too much scrolls inside the card.
  let summaryCard = $state<HTMLDivElement>();
  let summaryTight = $state(false);
  let summaryFit = $state<Overflow>({ scrolls: false, moreBelow: false });
  $effect(() => {
    const card = summaryCard;
    // read here, so that this runs again once the other layout is on the
    // page, and measures that one
    const tight = summaryTight;
    if (!card) return;
    return watchOverflow(card, (fit) => {
      if (needsPacking(fit, tight)) summaryTight = true;
      else summaryFit = fit;
    });
  });

  // A measure never unpacks the summary (it would then not fit, pack again,
  // and so on); a window of another size is a reason to try once more.
  function onWindowResize() {
    summaryTight = false;
  }

  function openExit() {
    sfx.click();
    showExit = true;
    // the next round must not appear, or the session end, under the dialog
    flow.setPaused(true);
  }

  function closeExit() {
    showExit = false;
    flow.setPaused(false);
  }

  async function exitSession() {
    showExit = false;
    await flow.exit();
    void goto("/");
  }

  function accuracyColor(a: number): string {
    if (a >= 0.8) return "#34d399";
    if (a >= 0.6) return "#fbbf24";
    return "#fb7185";
  }
</script>

<svelte:window onresize={onWindowResize} />

{#if view.phase === "loading"}
  <div class="flex h-[70vh] items-center justify-center">
    <div class="anim-float text-center">
      <div class="text-6xl" aria-hidden="true">🎴</div>
      <p class="mt-3 text-xl font-bold text-white/60">Preparando la mesa…</p>
    </div>
  </div>
{:else if view.phase === "error"}
  <!-- the error toast says what failed; this is the way on -->
  <div class="card mx-auto flex max-w-xl flex-col items-center gap-4 p-10 text-center">
    <p class="text-5xl" aria-hidden="true">😕</p>
    {#if view.session}
      <h1 class="text-3xl font-extrabold">No se ha podido cerrar la sesión</h1>
      <p class="text-lg text-white/60">Tus respuestas ya estaban guardadas. Puedes empezar otra cuando quieras.</p>
    {:else}
      <h1 class="text-3xl font-extrabold">No se ha podido preparar la sesión</h1>
      <p class="text-lg text-white/60">Prueba otra vez. Tu progreso está a salvo.</p>
    {/if}
    <div class="mt-2 flex w-full gap-3">
      <button
        class="pressable flex-1 rounded-2xl bg-gradient-to-r from-grape-500 to-fuchsia-500 py-3 text-lg font-extrabold hover:brightness-110"
        onclick={() => {
          sfx.click();
          onAgain();
        }}
      >
        {view.session ? "🔁 Otra" : "🔁 Reintentar"}
      </button>
      <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-extrabold hover:bg-white/20" onclick={() => void goto("/")}>
        🏠 Inicio
      </button>
    </div>
  </div>
{:else if view.phase === "empty"}
  <div class="card mx-auto flex max-w-xl flex-col items-center gap-4 p-10 text-center">
    <p class="text-5xl" aria-hidden="true">🌿</p>
    <h1 class="text-3xl font-extrabold">Nada que repasar ahora mismo</h1>
    {#if view.kind !== "standard"}
      <p class="text-lg text-white/60">Tus palabras están frescas. ¿Una sesión normal con alguna nueva?</p>
    {:else}
      <p class="text-lg text-white/60">Tus palabras están frescas. Vuelve dentro de un rato.</p>
    {/if}
    <div class="mt-2 flex w-full gap-3">
      <!-- from a normal session this would be a link to the page we are on -->
      {#if view.kind !== "standard"}
        <button
          class="pressable flex-1 rounded-2xl bg-gradient-to-r from-grape-500 to-fuchsia-500 py-3 text-lg font-extrabold hover:brightness-110"
          onclick={() => void goto("/play?kind=standard")}
        >
          🎯 Sesión normal
        </button>
      {/if}
      <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-extrabold hover:bg-white/20" onclick={() => void goto("/")}>
        🏠 Inicio
      </button>
    </div>
  </div>
{:else if view.phase === "playing" && view.session}
  <div class="relative mx-auto max-w-3xl">
    <!-- Top bar. A deep session has some forty rounds, more dots than the
      row has room for: then it is the dots that give way (closer together,
      and narrower), never the ✕ or the score beside them. -->
    <div class="mb-8 flex items-center gap-4">
      <button
        class="pressable flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-white/10 text-xl font-black hover:bg-white/20"
        aria-label="Salir de la sesión"
        onclick={openExit}
      >
        ✕
      </button>
      <div class="flex min-w-0 flex-1 items-center {view.session.rounds.length > 24 ? 'gap-1' : 'gap-1.5'}" aria-label="Progreso de la sesión">
        {#each view.session.rounds as _, i (i)}
          <span
            class="h-2 rounded-full transition-all duration-300
              {i < view.idx ? 'w-2 bg-lime-500/70' : i === view.idx ? 'w-6 bg-grape-400' : 'w-2 bg-white/15'}"
          ></span>
        {/each}
      </div>
      {#if view.combo > 1}
        <!-- it starts to glow at three in a row -->
        <span
          class="shrink-0 whitespace-nowrap rounded-full bg-orange-500/20 px-3.5 py-1.5 text-lg font-black text-orange-400"
          class:anim-glow={view.combo >= 3}
        >
          🔥 x{view.combo}
        </span>
      {/if}
      <span class="shrink-0 whitespace-nowrap rounded-full bg-grape-500/20 px-3.5 py-1.5 text-lg font-black text-grape-400">⚡ {view.xp} XP</span>
    </div>

    <!-- Round. The box can take focus (by script only) so that the keys go
      back to the round when the exit dialog closes. -->
    <div class="round-box" tabindex="-1" bind:this={roundBox}>
      {#key view.idx}
        <div class="anim-rise">
          {#if round}
            {#if round.type === "new_word"}
              <NewWord round={round} definitionLang={view.definitionLang} onAnswered={flow.answer} />
            {:else if round.type === "choice"}
              <Choice round={round} definitionLang={view.definitionLang} onAnswered={flow.answer} />
            {:else if round.type === "match"}
              <Match round={round} onAnswered={flow.answer} />
            {:else if round.type === "listen"}
              <Listen round={round} onAnswered={flow.answer} />
            {:else if round.type === "listen_type"}
              <ListenType round={round} onAnswered={flow.answer} />
            {:else if round.type === "build"}
              <Build round={round} onAnswered={flow.answer} />
            {:else if round.type === "cloze"}
              <Cloze round={round} onAnswered={flow.answer} />
            {:else if round.type === "conjugation"}
              <Conjugation round={round} onAnswered={flow.answer} />
            {:else if round.type === "review_card"}
              <ReviewCard round={round} onAnswered={flow.answer} />
            {/if}
          {/if}
        </div>
      {/key}
    </div>

    <!-- Exit confirm -->
    <Modal
      open={showExit}
      title="¿Salir de la sesión?"
      onclose={closeExit}
      returnFocus={() => roundBox?.querySelector<HTMLElement>("input:not([disabled])") ?? roundBox}
    >
      <p class="text-lg text-white/70">
        Tu progreso hasta ahora queda guardado. Aquí no se pierde nada — vuelve cuando quieras.
      </p>
      <div class="mt-6 flex gap-3">
        <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-bold hover:bg-white/20" onclick={closeExit}>
          Seguir jugando
        </button>
        <button class="pressable flex-1 rounded-2xl bg-coral-500 py-3 text-lg font-extrabold text-white hover:brightness-110" onclick={exitSession}>
          Salir
        </button>
      </div>
    </Modal>
  </div>
{:else if view.phase === "done" && view.summary}
  {@const summary = view.summary}
  {@const ring = accuracyRing(summary.rounds, summary.accuracy)}
  {@const time = sessionMinutes(summary.minutes)}
  <!-- Session summary. Never taller than the window (the 4rem is the
    layout's padding above and below), so that "Otra" and "Inicio" are always
    in view. When there is a lot to list, this is what gives way, in order:
    the ring shrinks; then the summary packs itself tight (`data-tight`: the
    spacing of a short window, and the ring beside the three figures); and
    what is still too much scrolls inside the card, whose lower edge fades
    out for as long as something is below it. -->
  <div
    class="mx-auto flex max-h-[calc(100vh-4rem)] max-w-2xl flex-col items-center gap-6 short:gap-3"
    data-tight={summaryTight ? "" : undefined}
  >
    <h1 class="anim-pop shrink-0 text-4xl font-extrabold short:text-3xl">¡Sesión completada! 🎊</h1>

    <!-- A box that scrolls must take the keyboard's focus, or only a mouse
      wheel reaches what is below: the arrow keys scroll what has the focus. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      bind:this={summaryCard}
      class="card flex min-h-0 w-full flex-col items-center gap-6 overflow-y-auto overflow-x-hidden p-8 short:gap-3 short:p-5"
      class:more-below={summaryFit.moreBelow}
      role={summaryFit.scrolls ? "group" : undefined}
      aria-label={summaryFit.scrolls ? "Resumen de la sesión" : undefined}
      tabindex={summaryFit.scrolls ? 0 : undefined}
    >
      <!-- No box of its own until the summary is packed tight: the ring and
        the figures are the card's own rows, and the ring is the one row
        that shrinks (its drawing stays round and centred in a flatter box). -->
      <div class="contents tight:flex tight:w-full tight:shrink-0 tight:items-center tight:gap-4">
        <svg
          class="h-45 min-h-28 w-45 shrink short:h-30 short:w-30 tight:shrink-0"
          viewBox="0 0 120 120"
          role="img"
          aria-label={ring.fill === null ? "Precisión: sin respuestas" : `Precisión ${ring.label}`}
        >
          <circle cx="60" cy="60" r="50" fill="none" stroke="rgba(255,255,255,0.08)" stroke-width="12" />
          <!-- no arc at all without an answer: a zero-length one still draws its round cap -->
          {#if ring.fill !== null}
            <circle
              class="anim-ring"
              cx="60"
              cy="60"
              r="50"
              fill="none"
              stroke={accuracyColor(summary.accuracy)}
              stroke-width="12"
              stroke-linecap="round"
              stroke-dasharray="{(ring.fill * 2 * Math.PI * 50).toFixed(1)} 999"
              transform="rotate(-90 60 60)"
            />
          {/if}
          <text x="60" y="66" text-anchor="middle" font-size="28" font-weight="800" fill="#fff">{ring.label}</text>
          <text x="60" y="84" text-anchor="middle" font-size="10" fill="rgba(255,255,255,0.5)">precisión</text>
        </svg>

        <div class="grid w-full shrink-0 grid-cols-3 gap-3 text-center tight:w-auto tight:min-w-0 tight:flex-1">
          <div class="rounded-2xl bg-white/5 p-4 short:p-2.5">
            <p class="text-3xl font-black text-grape-400">⚡ {summary.xp}</p>
            <p class="text-sm text-white/50">XP ganada</p>
          </div>
          <div class="rounded-2xl bg-white/5 p-4 short:p-2.5">
            <p class="text-3xl font-black text-orange-400">🔥 {summary.bestCombo}</p>
            <p class="text-sm text-white/50">mejor combo</p>
          </div>
          <div class="rounded-2xl bg-white/5 p-4 short:p-2.5">
            <p class="text-3xl font-black text-tubo-500">⏱ {time.count}</p>
            <p class="text-sm text-white/50">{time.unit}</p>
          </div>
        </div>
      </div>

      {#if summary.newWords.length > 0}
        <div class="w-full shrink-0">
          <p class="mb-2 text-center text-lg font-bold text-white/70">Palabras nuevas en tu colección ✨</p>
          <div class="flex flex-wrap justify-center gap-2">
            {#each summary.newWords as w (w)}
              <span class="anim-pop rounded-full bg-lime-500/20 px-4 py-1.5 text-lg font-bold text-lime-500">{w}</span>
            {/each}
          </div>
        </div>
      {/if}

      {#if summary.levelUp}
        <div class="anim-pop shrink-0 rounded-3xl bg-gradient-to-r from-mango-500 to-amber-500 px-8 py-4 text-center text-night-900 shadow-xl shadow-mango-500/30 short:py-2.5">
          <p class="text-2xl font-black">🎉 ¡Nivel {summary.levelUp.level}!</p>
          <p class="text-lg font-bold">{summary.levelUp.name}</p>
        </div>
      {/if}

      <!-- The ones a round gave (already toasted) and the ones the finish
        gave. A line each: a perfect first session earns three at once. -->
      {#if badges.length > 0}
        <ul class="flex w-full shrink-0 flex-col items-center gap-2">
          {#each badges as b (b.id)}
            <li class="anim-pop flex max-w-full items-center gap-2.5 rounded-2xl border border-mango-500/50 bg-mango-500/10 px-5 py-2 leading-6 short:py-1.5">
              <span class="anim-sparkle text-xl leading-6" aria-hidden="true">🏅</span>
              <p class="min-w-0">
                <span class="font-extrabold text-mango-500">{b.name}</span>
                <span class="text-sm text-white/60">· {b.description}</span>
              </p>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <div class="flex w-full shrink-0 gap-3">
      <button
        class="pressable flex-1 rounded-3xl bg-gradient-to-r from-grape-500 to-fuchsia-500 py-4 text-xl font-extrabold shadow-lg shadow-grape-500/30 hover:brightness-110 short:py-3"
        onclick={() => {
          sfx.click();
          onAgain();
        }}
      >
        🔁 Otra
      </button>
      <button
        class="pressable flex-1 rounded-3xl bg-white/10 py-4 text-xl font-extrabold hover:bg-white/20 short:py-3"
        onclick={() => {
          sfx.click();
          goto("/");
        }}
      >
        🏠 Inicio
      </button>
    </div>
  </div>
{/if}

<!-- Feedback pill: centred on the page column, not the window, and keyed so
  that each one pops in. It sits outside the phases so the last answer's
  pill gets its time over the summary. -->
{#if view.flash}
  {#key view.flash.seq}
    <div class="pointer-events-none fixed bottom-10 left-[var(--nav-w)] right-0 z-50 flex justify-center px-4">
      <div
        class="anim-pop max-w-xl text-balance break-words px-6 py-3 text-center font-black shadow-2xl
          {pillShape(view.flash.text)} {view.flash.correct ? 'bg-lime-500 text-night-900' : 'bg-coral-500 text-white'}"
        role="status"
      >
        {view.flash.text}
      </div>
    </div>
  {/key}
{/if}

<style>
  /* a ring round the whole round would say nothing: focus is put here only
     to take it off the ✕ */
  .round-box:focus {
    outline: none;
  }
</style>
