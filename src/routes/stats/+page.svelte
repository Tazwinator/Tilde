<script lang="ts">
  import { api } from "$lib/api";
  import { areaPath, columnXs, linePath, scaleYs, wordsChart } from "$lib/chart";
  import type { SeriesPoint, StatsData } from "$lib/contract";
  import { reportError } from "$lib/errors.svelte";

  // null while loading: nothing is shown under the title until the numbers
  // are in, so "no stats yet" can only mean that
  let stats = $state<StatsData | null>(null);
  // ...and if they never come, the page says so instead of staying empty
  let failed = $state(false);

  $effect(() => {
    void api.stats().then(
      (s) => (stats = s),
      (e) => {
        failed = true;
        reportError(e);
      },
    );
  });

  // nothing played at all, as opposed to a chart that is still short
  const fresh = $derived(stats !== null && stats.cumulativeWords.length === 0 && stats.accuracyAllTime === null);

  // the charts are 320 wide and 132 tall, with their labels underneath
  const FLOOR = 124;
  const xp = $derived.by(() => {
    const points = stats?.xpPerWeek ?? [];
    return { xs: columnXs(points.length), ys: scaleYs(points) };
  });
  const words = $derived.by(() => {
    const points = stats?.cumulativeWords ?? [];
    return { ...wordsChart(points), ys: scaleYs(points) };
  });

  // keyed by the round types the backend logs (session.rs)
  const GAME_LABELS: Record<string, string> = {
    choice: "Elección",
    listen: "Escucha",
    listen_type: "Dictado",
    match: "Parejas",
    build: "Construir",
    cloze: "Huecos",
    conjugation: "Conjugación",
    review_card: "Repasos",
  };

  function barHeight(v: number, points: SeriesPoint[], maxH: number): number {
    const max = Math.max(...points.map((p) => p.value), 1);
    return Math.max((v / max) * maxH, 2);
  }
</script>

<div class="flex flex-col gap-6">
  <header>
    <h1 class="text-4xl font-extrabold">Estadísticas 📈</h1>
    <p class="mt-1 text-lg text-white/60">Todo lo que has construido, semana a semana.</p>
  </header>

  {#if failed}
    <!-- the error toast says what failed and offers to reload; this is what is left when it is closed -->
    <div class="card flex h-64 items-center justify-center p-8 text-center">
      <div>
        <p class="text-5xl" aria-hidden="true">😕</p>
        <p class="mt-3 text-lg text-white/60">No se han podido cargar las estadísticas.</p>
      </div>
    </div>
  {:else if fresh}
    <div class="card flex h-64 items-center justify-center p-8 text-center">
      <div>
        <p class="anim-float text-5xl" aria-hidden="true">📈</p>
        <p class="mt-3 text-lg text-white/60">Aún no hay estadísticas — ¡juega tu primera sesión y este panel cobrará vida!</p>
      </div>
    </div>
  {:else if stats}
    <!-- No each block here is keyed: a key that came up twice (the same
      day's label in two years, once) took the whole page down. -->
    <div class="grid gap-5 md:grid-cols-2">
      <!-- Minutes per week -->
      <div class="card p-6">
        <h2 class="text-xl font-extrabold">⏱ Minutos por semana</h2>
        <svg viewBox="0 0 320 140" class="mt-3 w-full" role="img" aria-label="Minutos de juego por semana">
          {#each stats.minutesPerWeek as p, i}
            {@const barW = 320 / stats.minutesPerWeek.length}
            <rect
              x={i * barW + barW * 0.18}
              y={130 - barHeight(p.value, stats.minutesPerWeek, 110)}
              width={barW * 0.64}
              height={barHeight(p.value, stats.minutesPerWeek, 110)}
              rx="5"
              fill="url(#gradMin)"
            />
            <text x={i * barW + barW / 2} y="140" text-anchor="middle" font-size="10" fill="rgba(255,255,255,0.45)">{p.label}</text>
          {/each}
          <defs>
            <linearGradient id="gradMin" x1="0" y1="1" x2="0" y2="0">
              <stop offset="0%" stop-color="#7c3aed" />
              <stop offset="100%" stop-color="#38bdf8" />
            </linearGradient>
          </defs>
        </svg>
      </div>

      <!-- XP per week -->
      <div class="card p-6">
        <h2 class="text-xl font-extrabold">⚡ XP por semana</h2>
        <svg viewBox="0 0 320 140" class="mt-3 w-full" role="img" aria-label="XP ganada por semana">
          <path d={areaPath(xp.xs, xp.ys, FLOOR)} fill="rgba(124,58,237,0.25)" />
          <path d={linePath(xp.xs, xp.ys)} fill="none" stroke="#a78bfa" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" />
          <!-- each point over the middle of its week, like the bars beside it -->
          {#each stats.xpPerWeek as p, i}
            <circle cx={xp.xs[i]} cy={xp.ys[i]} r="3.5" fill="#a78bfa" />
            <text x={xp.xs[i]} y="140" text-anchor="middle" font-size="10" fill="rgba(255,255,255,0.45)">{p.label}</text>
          {/each}
        </svg>
      </div>

      <!-- Cumulative words -->
      <div class="card p-6">
        <h2 class="text-xl font-extrabold">📖 Palabras que conoces</h2>
        {#if words.xs.length === 0}
          <p class="mt-3 text-base text-white/50">Juega una sesión y aquí verás crecer tus palabras ✨</p>
        {:else}
          {@const last = words.xs.length - 1}
          <svg viewBox="0 0 320 140" class="mt-3 w-full" role="img" aria-label="Palabras acumuladas">
            {#if last > 0}
              <path d={areaPath(words.xs, words.ys, FLOOR)} fill="rgba(52,211,153,0.2)" />
              <path d={linePath(words.xs, words.ys)} fill="none" stroke="#34d399" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" />
            {/if}
            <!-- one dot, on the latest count: a dot per day soon runs into a smear -->
            <circle cx={words.xs[last]} cy={words.ys[last]} r="3.5" fill="#34d399" />
            {#each words.ticks as t}
              <text x={t.x} y="140" text-anchor={t.anchor} font-size="10" fill="rgba(255,255,255,0.45)">{t.label}</text>
            {/each}
          </svg>
          {#if last === 0}
            <p class="mt-1 text-center text-base text-white/50">Vuelve otro día y verás tu curva</p>
          {/if}
        {/if}
      </div>

      <!-- Accuracy + game mix -->
      <div class="card flex flex-col gap-5 p-6">
        <div>
          <h2 class="text-xl font-extrabold">🎯 Precisión total</h2>
          {#if stats.accuracyAllTime === null}
            <p class="mt-2 text-7xl font-black text-white/40">
              <span aria-hidden="true">—</span><span class="sr-only">sin datos todavía</span>
            </p>
          {:else}
            <p class="mt-2 text-7xl font-black text-lime-500">{Math.round(stats.accuracyAllTime * 100)}%</p>
          {/if}
        </div>
        <div>
          <h3 class="mb-2 text-lg font-extrabold">🎲 Tu mezcla de juegos</h3>
          {#if stats.gameMix.every((g) => g.value === 0)}
            <p class="text-base text-white/50">Juega tu primera sesión y esto se llena solito ✨</p>
          {/if}
          {#each stats.gameMix as g}
            {@const total = Math.max(stats!.gameMix.reduce((a, b) => a + b.value, 0), 1)}
            <div class="mb-2">
              <div class="flex justify-between text-sm font-bold">
                <span>{GAME_LABELS[g.label] ?? g.label}</span>
                <span class="text-white/50">{Math.round((g.value / total) * 100)}%</span>
              </div>
              <div class="mt-1 h-3 overflow-hidden rounded-full bg-white/8">
                <div
                  class="h-full rounded-full bg-gradient-to-r from-grape-500 to-tubo-500 transition-all duration-700"
                  style="width: {(g.value / total) * 100}%"
                ></div>
              </div>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>
