<script lang="ts">
  import { api } from "$lib/api";
  import type { SeriesPoint, StatsData } from "$lib/contract";

  let stats = $state<StatsData | null>(null);

  $effect(() => {
    void api.stats().then((s) => (stats = s));
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

  function linePath(points: SeriesPoint[], w: number, h: number, pad = 8): string {
    if (points.length < 2) return "";
    const max = Math.max(...points.map((p) => p.value), 1);
    const step = (w - pad * 2) / (points.length - 1);
    return points
      .map((p, i) => {
        const x = pad + i * step;
        const y = h - pad - (p.value / max) * (h - pad * 2);
        return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  }

  function areaPath(points: SeriesPoint[], w: number, h: number, pad = 8): string {
    if (points.length < 2) return "";
    const line = linePath(points, w, h, pad);
    return `${line} L${(w - pad).toFixed(1)},${h - pad} L${pad},${h - pad} Z`;
  }

  // Line-chart points sit at the very edges, so their labels hang inward.
  function anchor(i: number, n: number): "start" | "middle" | "end" {
    return i === 0 ? "start" : i === n - 1 ? "end" : "middle";
  }

  function barHeight(v: number, points: SeriesPoint[], maxH: number): number {
    const max = Math.max(...points.map((p) => p.value), 1);
    return Math.max((v / max) * maxH, 2);
  }
</script>

<div class="flex flex-col gap-6">
  <header>
    <h1 class="text-4xl font-extrabold">Stats 📈</h1>
    <p class="mt-1 text-lg text-white/60">Todo lo que has construido, semana a semana.</p>
  </header>

  {#if stats}
    <div class="grid gap-5 md:grid-cols-2">
      <!-- Minutes per week -->
      <div class="card p-6">
        <h2 class="text-xl font-extrabold">⏱ Minutos por semana</h2>
        <svg viewBox="0 0 320 140" class="mt-3 w-full" role="img" aria-label="Minutos de juego por semana">
          {#each stats.minutesPerWeek as p, i (p.label)}
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
          <path d={areaPath(stats.xpPerWeek, 320, 132)} fill="rgba(124,58,237,0.25)" />
          <path d={linePath(stats.xpPerWeek, 320, 132)} fill="none" stroke="#a78bfa" stroke-width="3" stroke-linecap="round" />
          {#each stats.xpPerWeek as p, i (p.label)}
            {@const x = 8 + (i * (320 - 16)) / (stats.xpPerWeek.length - 1)}
            {@const y = 132 - 8 - (p.value / Math.max(...stats.xpPerWeek.map((q) => q.value), 1)) * 116}
            <circle cx={x} cy={y} r="3.5" fill="#a78bfa" />
            <text x={x} y="140" text-anchor={anchor(i, stats.xpPerWeek.length)} font-size="10" fill="rgba(255,255,255,0.45)">{p.label}</text>
          {/each}
        </svg>
      </div>

      <!-- Cumulative words -->
      <div class="card p-6">
        <h2 class="text-xl font-extrabold">📖 Palabras que conoces</h2>
        <svg viewBox="0 0 320 140" class="mt-3 w-full" role="img" aria-label="Palabras acumuladas">
          <path d={areaPath(stats.cumulativeWords, 320, 132)} fill="rgba(52,211,153,0.2)" />
          <path d={linePath(stats.cumulativeWords, 320, 132)} fill="none" stroke="#34d399" stroke-width="3" stroke-linecap="round" />
          {#each stats.cumulativeWords as p, i (p.label)}
            {@const x = 8 + (i * (320 - 16)) / (stats.cumulativeWords.length - 1)}
            {@const y = 132 - 8 - (p.value / Math.max(...stats.cumulativeWords.map((q) => q.value), 1)) * 116}
            <circle cx={x} cy={y} r="3.5" fill="#34d399" />
            <text x={x} y="140" text-anchor={anchor(i, stats.cumulativeWords.length)} font-size="10" fill="rgba(255,255,255,0.45)">{p.label}</text>
          {/each}
        </svg>
      </div>

      <!-- Accuracy + game mix -->
      <div class="card flex flex-col gap-5 p-6">
        <div>
          <h2 class="text-xl font-extrabold">🎯 Precisión total</h2>
          <p class="mt-2 text-7xl font-black text-lime-500">{Math.round(stats.accuracyAllTime * 100)}%</p>
        </div>
        <div>
          <h3 class="mb-2 text-lg font-extrabold">🎲 Tu mezcla de juegos</h3>
          {#if stats.gameMix.every((g) => g.value === 0)}
            <p class="text-base text-white/50">Juega tu primera sesión y esto se llena solito ✨</p>
          {/if}
          {#each stats.gameMix as g (g.label)}
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
  {:else}
    <div class="card flex h-64 items-center justify-center p-8 text-center">
      <div>
        <p class="anim-float text-5xl" aria-hidden="true">📈</p>
        <p class="mt-3 text-lg text-white/60">Aún no hay estadísticas — ¡juega tu primera sesión y este panel cobrará vida!</p>
      </div>
    </div>
  {/if}
</div>
