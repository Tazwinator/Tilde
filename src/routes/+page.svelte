<script lang="ts">
  import { goto } from "$app/navigation";
  import { fade } from "svelte/transition";
  import { api } from "$lib/api";
  import { sfx } from "$lib/sfx";
  import type { Badge, Profile } from "$lib/contract";

  let profile = $state<Profile | null>(null);
  let badges = $state<Badge[]>([]);
  let loaded = $state(false);

  const CARDS = [
    { kind: "quick", emoji: "⚡", title: "Sesión rápida", desc: "3 min · un café de aprendizaje", mins: 3, accent: "from-amber-400 to-orange-500" },
    { kind: "standard", emoji: "🎯", title: "Sesión normal", desc: "6 min · el dulce punto", mins: 6, accent: "from-grape-500 to-fuchsia-500" },
    { kind: "deep", emoji: "🌊", title: "Sesión profunda", desc: "12 min · sumérgete del todo", mins: 12, accent: "from-sky-500 to-indigo-500" },
    { kind: "review_only", emoji: "🔁", title: "Solo repaso", desc: "Repasa lo que ya casi sabes", mins: 4, accent: "from-emerald-400 to-teal-500" },
  ] as const;

  $effect(() => {
    void load();
  });

  async function load() {
    const [p, b] = await Promise.all([api.profile(), api.listBadges()]);
    profile = p;
    badges = b;
    loaded = true;
    if (await api.placementStatus() === null) {
      goto("/placement");
    }
  }

  function play(kind: string) {
    sfx.click();
    void goto(`/play?kind=${kind}`);
  }
</script>

{#if loaded && profile}
  <div class="flex flex-col gap-8">
    <!-- Header -->
    <header class="card anim-rise p-6">
      <div class="flex flex-wrap items-center gap-5">
        <div
          class="flex h-20 w-20 shrink-0 flex-col items-center justify-center rounded-3xl bg-gradient-to-br from-grape-500 to-fuchsia-500 text-white shadow-lg shadow-grape-500/40"
        >
          <span class="text-3xl font-black leading-none">{profile.level}</span>
          <span class="text-[11px] font-semibold uppercase tracking-wider opacity-90">nivel</span>
        </div>
        <div class="min-w-0 flex-1">
          <h1 class="text-3xl font-extrabold tracking-tight">
            ¡Hola! <span class="text-grape-400">Nivel {profile.level} · {profile.levelName}</span>
          </h1>
          <div class="mt-2 h-4 overflow-hidden rounded-full bg-white/10" role="progressbar" aria-valuenow={Math.round(profile.levelProgress * 100)} aria-valuemin={0} aria-valuemax={100} aria-label="Progreso de nivel">
            <div
              class="h-full rounded-full bg-gradient-to-r from-grape-400 via-fuchsia-400 to-tubo-500 transition-all duration-700"
              style="width: {Math.round(profile.levelProgress * 100)}%"
            ></div>
          </div>
          <p class="mt-1.5 text-sm text-white/60">{profile.xp} XP · {Math.round(profile.levelProgress * 100)}% hacia el siguiente nivel</p>
        </div>
        <div class="flex flex-wrap gap-2.5">
          <span class="rounded-full bg-tubo-500/20 px-4 py-1.5 text-base font-bold text-tubo-500" title="Estimación CEFR">MCER {profile.cefrEstimate}</span>
          <span class="rounded-full bg-white/10 px-4 py-1.5 text-base font-bold" title="Minutos totales">⏱ {profile.minutesTotal} min</span>
          <span class="rounded-full bg-lime-500/20 px-4 py-1.5 text-base font-bold text-lime-500" title="Palabras conocidas">📖 {profile.wordsKnown} palabras</span>
        </div>
      </div>
    </header>

    <!-- Welcome back banner -->
    {#if profile.daysSinceLastSession >= 3}
      <section class="card anim-rise flex items-center gap-4 border-grape-400/30 bg-gradient-to-r from-grape-500/20 to-transparent p-6" in:fade={{ duration: 300 }}>
        <span class="anim-float text-5xl" aria-hidden="true">👋</span>
        <div class="flex-1">
          <h2 class="text-2xl font-extrabold">¡Qué gusto verte!</h2>
          <p class="text-lg text-white/80">
            Your {profile.wordsKnown} words are safe and waiting. A 4-minute session is ready.
          </p>
        </div>
        <button class="pressable rounded-2xl bg-lime-500 px-6 py-3 text-lg font-extrabold text-night-900 shadow-lg shadow-lime-500/30 hover:brightness-110" onclick={() => play("review_only")}>
          Empezar →
        </button>
      </section>
    {/if}

    <!-- Play cards -->
    <section aria-label="Sesiones de juego">
      <h2 class="mb-4 text-2xl font-extrabold">Elige tu aventura</h2>
      <div class="grid gap-4 sm:grid-cols-2">
        {#each CARDS as c (c.kind)}
          <button
            class="card card-hover pressable group flex items-center gap-4 p-5 text-left"
            onclick={() => play(c.kind)}
          >
            <span class="flex h-16 w-16 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br {c.accent} text-3xl shadow-lg" aria-hidden="true">{c.emoji}</span>
            <span class="min-w-0 flex-1">
              <span class="block text-xl font-extrabold">{c.title}</span>
              <span class="block text-base text-white/60">{c.desc}</span>
              <span class="mt-1 flex gap-2 text-sm font-semibold">
                <span class="rounded-full bg-coral-500/20 px-2.5 py-0.5 text-coral-500">🔔 {profile.reviewsDue} repasos</span>
                <span class="rounded-full bg-tubo-500/20 px-2.5 py-0.5 text-tubo-500">✨ {profile.newWordsReady} nuevas</span>
              </span>
            </span>
            <span class="text-2xl text-white/30 transition-transform group-hover:translate-x-1" aria-hidden="true">→</span>
          </button>
        {/each}

        <button
          class="card card-hover pressable group flex items-center gap-4 p-5 text-left sm:col-span-2"
          onclick={() => {
            sfx.click();
            goto("/sidecar");
          }}
        >
          <span class="flex h-16 w-16 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-slate-600 to-slate-800 text-3xl shadow-lg" aria-hidden="true">🍿</span>
          <span class="min-w-0 flex-1">
            <span class="block text-xl font-extrabold">Modo secundario</span>
            <span class="block text-base text-white/60">Juega mientras ves una peli · ventana flotante, cero distracciones</span>
          </span>
          <span class="text-2xl text-white/30 transition-transform group-hover:translate-x-1" aria-hidden="true">→</span>
        </button>
      </div>
    </section>

    <!-- Badges strip -->
    <section aria-label="Insignias">
      <h2 class="mb-4 text-2xl font-extrabold">Insignias <span class="text-lg font-semibold text-white/50">({badges.filter((b) => b.earnedAt != null).length}/{badges.length})</span></h2>
      <div class="flex flex-wrap gap-3">
        {#each badges as b (b.id)}
          {@const earned = b.earnedAt != null}
          <div
            class="flex items-center gap-2.5 rounded-2xl border px-4 py-2.5 {earned ? 'border-mango-500/50 bg-mango-500/10' : 'border-white/10 bg-white/[0.03] opacity-50 grayscale'}"
            title="{b.description}{earned ? '' : ' · bloqueada'}"
          >
            <span class="text-2xl {earned ? 'anim-sparkle' : ''}" aria-hidden="true">{earned ? "🏅" : "🔒"}</span>
            <span class="font-bold {earned ? 'text-mango-500' : 'text-white/60'}">{b.name}</span>
          </div>
        {/each}
      </div>
    </section>

    <!-- Footer nudge (positive framing) -->
    <p class="pb-4 text-center text-lg text-white/50">
      Cada palabra cuenta · llevas <strong class="text-white/80">{profile.sessionsTotal} sesiones</strong> jugadas 🎉
    </p>
  </div>
{:else}
  <div class="flex h-[60vh] items-center justify-center">
    <div class="anim-float text-6xl" aria-hidden="true">~</div>
  </div>
{/if}
