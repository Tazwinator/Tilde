<script lang="ts">
  import "../app.css";
  import { fly } from "svelte/transition";
  import type { Snippet } from "svelte";
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { forgetLegacySoundFlag, setSfxEnabled, sfx } from "$lib/sfx";
  import { motion } from "$lib/motion";
  import { failure, reportError } from "$lib/errors.svelte";
  import { dismissToast, toasts } from "$lib/celebrate.svelte";

  let { children }: { children: Snippet } = $props();

  // a failed call nobody caught still gets said out loud, with a way out
  onMount(() => {
    const onRejection = (e: PromiseRejectionEvent) => reportError(e.reason);
    window.addEventListener("unhandledrejection", onRejection);
    return () => window.removeEventListener("unhandledrejection", onRejection);
  });

  // Sound follows the saved setting and nothing else. Every start comes
  // through here, and so do a reset and a restore, which end in a reload.
  onMount(() => {
    forgetLegacySoundFlag();
    void api.getSettings().then((s) => setSfxEnabled(s.soundEnabled));
  });

  interface NavItem {
    href: string;
    emoji: string;
    label: string;
  }

  const NAV: NavItem[] = [
    { href: "/", emoji: "🏠", label: "Inicio" },
    { href: "/play", emoji: "🎮", label: "Jugar" },
    { href: "/explore", emoji: "🔍", label: "Explorar" },
    { href: "/sentences", emoji: "💬", label: "Frases" },
    { href: "/stats", emoji: "📈", label: "Estadísticas" },
    { href: "/settings", emoji: "⚙️", label: "Ajustes" },
  ];

  const isShellRoute = $derived(page.route?.id !== "/sidecar");
  const activeIdx = $derived.by(() => {
    const p = page.url.pathname;
    if (p.startsWith("/play")) return 1;
    const idx = NAV.findIndex((n) => n.href === p);
    return idx === -1 ? 0 : idx;
  });

  // The bar beside the active link is one element that slides between them.
  // Its place is measured from the link itself, not worked out from an
  // assumed row height.
  let links: HTMLAnchorElement[] = $state([]);
  let barY = $state(0);
  let barSlides = $state(false);

  $effect(() => {
    const el = links[activeIdx];
    if (el) barY = el.offsetTop;
  });

  // The slide is switched on only after the bar has been painted where it
  // belongs: the app can open on any page, and the bar must just be there,
  // not arrive from "Inicio". Two frames, because a transition switched on
  // before that first paint still runs from the bar's starting place.
  onMount(() => {
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => (barSlides = true));
    });
    return () => cancelAnimationFrame(frame);
  });
</script>

{#if isShellRoute}
  <div class="flex min-h-screen">
    <nav
      class="fixed bottom-0 left-0 top-0 z-40 flex w-[var(--nav-w)] flex-col gap-2 border-r border-white/10 bg-white/[0.03] px-2.5 py-5 backdrop-blur md:px-4"
      aria-label="Navegación principal"
    >
      <a
        href="/"
        class="pressable mb-6 flex items-center gap-2.5 px-1 md:px-2"
        onclick={() => sfx.click()}
        aria-label="Tilde · inicio"
      >
        <span
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-grape-500 to-tubo-500 text-2xl font-black text-white shadow-lg shadow-grape-500/40"
          aria-hidden="true"
        >
          ~
        </span>
        <span class="hidden text-2xl font-extrabold tracking-tight md:inline">Tilde</span>
      </a>

      <div class="relative flex flex-col gap-2">
        <div
          class="absolute left-0 top-0 h-14 w-1.5 rounded-full bg-gradient-to-b from-grape-400 to-tubo-500
            {barSlides ? 'transition-transform duration-300 ease-out' : ''}"
          style="transform: translateY({barY}px)"
          aria-hidden="true"
        ></div>
        {#each NAV as item, i (item.href)}
          <a
            bind:this={links[i]}
            href={item.href}
            class="pressable relative flex h-14 items-center gap-3 rounded-2xl px-2.5 text-lg transition-colors md:px-3
              {i === activeIdx
              ? 'bg-white/10 font-bold text-white'
              : 'text-white/60 hover:bg-white/5 hover:text-white'}"
            aria-current={i === activeIdx ? "page" : undefined}
            onclick={() => sfx.click()}
          >
            <span class="w-6 text-center text-xl" aria-hidden="true">{item.emoji}</span>
            <span class="hidden md:inline">{item.label}</span>
          </a>
        {/each}
      </div>

      <div class="mt-auto hidden px-3 text-sm text-white/40 md:block">
        <p>español · es-ES</p>
      </div>
    </nav>

    <main class="ml-[var(--nav-w)] flex-1">
      {#key page.url.pathname}
        <!-- No out-transition: it kept the old page in the document, stacked
          on the new one, for as long as it ran. -->
        <div class="mx-auto max-w-4xl px-5 py-8 md:px-10" in:fly={{ y: 14, duration: motion(220) }}>
          {@render children()}
        </div>
      {/key}
    </main>
  </div>
{:else}
  {@render children()}
{/if}

<!-- Badge toasts: above the page and the feedback pill, below dialogs and
  the error toast. The region is always there so a screen reader hears what
  is added to it; only the toasts themselves take clicks. -->
<div
  class="pointer-events-none fixed right-0 top-6 z-[60] flex flex-col items-center gap-2 px-4 {isShellRoute ? 'left-[var(--nav-w)]' : 'left-0'}"
  role="status"
  aria-live="polite"
>
  {#each toasts as t (t.id)}
    <button
      class="anim-pop pointer-events-auto rounded-2xl bg-night-800 text-left shadow-2xl"
      title="Cerrar"
      onclick={() => dismissToast(t.id)}
    >
      <span class="flex items-center gap-3 rounded-2xl border border-mango-500/50 bg-mango-500/10 px-6 py-3">
        <span class="anim-sparkle text-3xl" aria-hidden="true">🏅</span>
        <span>
          <span class="block font-extrabold text-mango-500">{t.badge.name}</span>
          <span class="block text-sm text-white/60">{t.badge.description}</span>
        </span>
      </span>
    </button>
  {/each}
</div>

{#if failure.message}
  <div
    class="anim-pop fixed bottom-6 left-1/2 z-[9800] flex w-[min(40rem,calc(100%-2rem))] -translate-x-1/2 items-start gap-4 rounded-3xl bg-coral-500 px-6 py-4 text-white shadow-2xl"
    role="alert"
  >
    <div class="min-w-0 flex-1">
      <p class="text-lg font-extrabold">Algo ha fallado 😕</p>
      <p class="mt-0.5 break-words text-base text-white/90">{failure.message}</p>
    </div>
    <button class="pressable shrink-0 rounded-2xl bg-white/20 px-4 py-2 font-bold hover:bg-white/30" onclick={() => location.reload()}>
      Recargar
    </button>
    <button class="shrink-0 px-1 py-2 text-xl font-black text-white/80 hover:text-white" aria-label="Cerrar" onclick={() => (failure.message = null)}>
      ✕
    </button>
  </div>
{/if}
