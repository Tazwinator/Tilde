<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    open,
    title,
    onclose,
    children,
    danger = false,
  }: {
    open: boolean;
    title: string;
    onclose: () => void;
    children: Snippet;
    danger?: boolean;
  } = $props();

  let panel = $state<HTMLDivElement>();

  // While open, every key press belongs to the dialog. Stopping it on its way
  // down, at the window, keeps it from the window listeners the page behind
  // uses for shortcuts (a round answering to 1–4); a focused button still
  // clicks on Enter or Space, as that's the browser's default action.
  function onkeydowncapture(e: KeyboardEvent) {
    if (!open) return;
    e.stopPropagation();
    if (e.key === "Escape") onclose();
    if (e.key === "Tab" && panel) {
      // keep focus inside the dialog
      const items = [...panel.querySelectorAll<HTMLElement>("button:not([disabled]), input, select, textarea, a[href]")];
      const at = items.indexOf(document.activeElement as HTMLElement);
      const next = at === -1 ? 0 : (at + (e.shiftKey ? items.length - 1 : 1)) % items.length;
      e.preventDefault();
      items[next]?.focus();
    }
  }

  // Focus starts on the first button, which is always the safe choice, and
  // goes back where it was when the dialog closes.
  $effect(() => {
    if (!open || !panel) return;
    const before = document.activeElement;
    panel.querySelector("button")?.focus();
    return () => {
      if (before instanceof HTMLElement && before.isConnected) before.focus();
    };
  });
</script>

<svelte:window {onkeydowncapture} />

{#if open}
  <div class="fixed inset-0 z-[9000] flex items-center justify-center" role="dialog" aria-modal="true" aria-label={title}>
    <button
      type="button"
      tabindex="-1"
      class="absolute inset-0 cursor-default bg-black/60 backdrop-blur-sm"
      aria-label="Cerrar"
      onclick={onclose}
    ></button>
    <div class="card anim-pop relative w-[min(92vw,26rem)] p-6" bind:this={panel}>
      <h2 class="mb-4 text-2xl font-extrabold" class:text-coral-500={danger}>{title}</h2>
      {@render children()}
    </div>
  </div>
{/if}
