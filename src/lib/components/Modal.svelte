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

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) onclose();
  }
</script>

<svelte:window {onkeydown} />

{#if open}
  <div class="fixed inset-0 z-[9000] flex items-center justify-center" role="dialog" aria-modal="true" aria-label={title}>
    <button
      type="button"
      class="absolute inset-0 cursor-default bg-black/60 backdrop-blur-sm"
      aria-label="Cerrar"
      onclick={onclose}
    ></button>
    <div class="card anim-pop relative w-[min(92vw,26rem)] p-6">
      <h2 class="mb-4 text-2xl font-extrabold" class:text-coral-500={danger}>{title}</h2>
      {@render children()}
    </div>
  </div>
{/if}
