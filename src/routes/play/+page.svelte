<script lang="ts">
  import { page } from "$app/state";
  import PlaySession from "./PlaySession.svelte";

  // Every session gets a PlaySession of its own. Another `?kind=` (the nav's
  // bare "Jugar" while a quick session is up) or "Otra" changes the key, and
  // the old instance is torn down whole: there is no state to reset by hand.
  const requested = $derived(page.url.searchParams.get("kind"));
  let run = $state(0);
</script>

{#key `${requested}|${run}`}
  <PlaySession {requested} onAgain={() => (run += 1)} />
{/key}
