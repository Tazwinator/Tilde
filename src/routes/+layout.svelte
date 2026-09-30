<script lang="ts">
  import "../app.css";
  import { fade, fly } from "svelte/transition";
  import type { Snippet } from "svelte";
  import { page } from "$app/state";
  import { sfx } from "$lib/sfx";

  let { children }: { children: Snippet } = $props();

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
    { href: "/stats", emoji: "📈", label: "Stats" },
    { href: "/settings", emoji: "⚙️", label: "Ajustes" },
  ];

  const isShellRoute = $derived(page.route?.id !== "/sidecar");
  const activeIdx = $derived.by(() => {
    const p = page.url.pathname;
    if (p.startsWith("/play")) return 1;
    const idx = NAV.findIndex((n) => n.href === p);
    return idx === -1 ? 0 : idx;
  });
</script>

{#if isShellRoute}
  <div class="flex min-h-screen">
    <nav
      class="fixed bottom-0 left-0 top-0 z-40 flex w-[72px] flex-col gap-2 border-r border-white/10 bg-white/[0.03] px-2.5 py-5 backdrop-blur md:w-60 md:px-4"
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
        {#each NAV as item, i (item.href)}
          {#if i === activeIdx}
            <div
              class="absolute left-0 h-14 w-1.5 rounded-full bg-gradient-to-b from-grape-400 to-tubo-500 transition-all duration-300 ease-out"
              style="top: {i * 60}px"
              aria-hidden="true"
            ></div>
          {/if}
          <a
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

    <main class="ml-[72px] flex-1 md:ml-60">
      {#key page.url.pathname}
        <div class="mx-auto max-w-4xl px-5 py-8 md:px-10" in:fly={{ y: 14, duration: 220 }} out:fade={{ duration: 100 }}>
          {@render children()}
        </div>
      {/key}
    </main>
  </div>
{:else}
  {@render children()}
{/if}
