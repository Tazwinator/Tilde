<script lang="ts">
  import { api } from "$lib/api";
  import { setSfxEnabled } from "$lib/sfx";
  import Modal from "$lib/components/Modal.svelte";
  import type { Settings, TtsInfo } from "$lib/contract";

  let settings = $state<Settings | null>(null);
  let tts = $state<TtsInfo | null>(null);
  let credits = $state("");
  let backupPath = $state<string | null>(null);
  let savedFlash = $state(false);
  let restoreFile = $state<File | null>(null);
  let showRestore = $state(false);
  let showReset = $state(false);
  let showReset2 = $state(false);
  let resetDone = $state(false);
  let fileInput: HTMLInputElement | undefined = $state();

  $effect(() => {
    void (async () => {
      settings = await api.getSettings();
      tts = await api.ttsInfo();
      credits = await api.contentCredits();
    })();
  });

  async function save() {
    if (!settings) return;
    settings = await api.setSettings({ ...settings });
    setSfxEnabled(settings.soundEnabled);
    savedFlash = true;
    setTimeout(() => (savedFlash = false), 1500);
  }

  async function exportBackup() {
    backupPath = await api.exportBackup();
  }

  function onRestoreFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    restoreFile = input.files?.[0] ?? null;
    if (restoreFile) showRestore = true;
    input.value = "";
  }

  async function doRestore() {
    showRestore = false;
    if (!restoreFile) return;
    const bytes = new Uint8Array(await restoreFile.arrayBuffer());
    await api.importBackup(bytes);
    restoreFile = null;
    savedFlash = true;
    setTimeout(() => (savedFlash = false), 1500);
  }

  async function doReset() {
    showReset2 = false;
    resetDone = true;
    await api.resetProgress();
    setTimeout(() => {
      resetDone = false;
      location.href = "/";
    }, 1600);
  }
</script>

<div class="flex flex-col gap-6">
  <header>
    <h1 class="text-4xl font-extrabold">Ajustes ⚙️</h1>
    <p class="mt-1 text-lg text-white/60">Afinemos tu tilde.</p>
  </header>

  {#if settings}
    <!-- Definition language -->
    <section class="card p-6">
      <h2 class="text-2xl font-extrabold">Idioma de definiciones</h2>
      <p class="mt-1 text-base text-white/60">¿Prefieres las traducciones en inglés o en español?</p>
      <div class="mt-4 flex gap-3">
        <button
          class="pressable rounded-2xl px-6 py-3 text-lg font-extrabold transition-colors
            {settings.definitionLang === 'en' ? 'bg-grape-500 text-white' : 'bg-white/10 text-white/60 hover:bg-white/20'}"
          onclick={() => {
            settings!.definitionLang = "en";
            void save();
          }}
        >
          🇬🇧 English primero
        </button>
        <button
          class="pressable rounded-2xl px-6 py-3 text-lg font-extrabold transition-colors
            {settings.definitionLang === 'es' ? 'bg-grape-500 text-white' : 'bg-white/10 text-white/60 hover:bg-white/20'}"
          onclick={() => {
            settings!.definitionLang = "es";
            void save();
          }}
        >
          🇪🇸 Español primero
        </button>
      </div>
    </section>

    <!-- Session length -->
    <section class="card p-6">
      <h2 class="text-2xl font-extrabold">Sesión por defecto</h2>
      <div class="mt-4 flex gap-3">
        {#each [{ id: "quick", label: "Rápida · 3 min" }, { id: "standard", label: "Normal · 6 min" }, { id: "deep", label: "Profunda · 12 min" }] as opt (opt.id)}
          <button
            class="pressable rounded-2xl px-5 py-3 text-lg font-bold transition-colors
              {settings.sessionLength === opt.id ? 'bg-grape-500 text-white' : 'bg-white/10 text-white/60 hover:bg-white/20'}"
            onclick={() => {
              settings!.sessionLength = opt.id;
              void save();
            }}
          >
            {opt.label}
          </button>
        {/each}
      </div>
    </section>

    <!-- Sound -->
    <section class="card flex items-center justify-between p-6">
      <div>
        <h2 class="text-2xl font-extrabold">🔊 Sonidos</h2>
        <p class="mt-1 text-base text-white/60">Chispitas sintéticas al acertar (100% local).</p>
      </div>
      <button
        role="switch"
        aria-checked={settings.soundEnabled}
        aria-label="Activar o desactivar sonidos"
        class="relative h-9 w-16 shrink-0 rounded-full transition-colors {settings.soundEnabled ? 'bg-lime-500' : 'bg-white/15'}"
        onclick={() => {
          settings!.soundEnabled = !settings!.soundEnabled;
          void save();
        }}
      >
        <span
          class="absolute top-1 h-7 w-7 rounded-full bg-white shadow transition-all {settings.soundEnabled ? 'left-8' : 'left-1'}"
        ></span>
      </button>
    </section>

    <!-- Target language + TTS -->
    <section class="card p-6">
      <h2 class="text-2xl font-extrabold">🌍 Idioma objetivo</h2>
      <p class="mt-2 inline-block rounded-full bg-tubo-500/15 px-4 py-1.5 text-lg font-bold text-tubo-500">Español (España)</p>
      <p class="mt-2 text-base text-white/50">más idiomas pronto…</p>
      <div class="mt-4 border-t border-white/10 pt-4">
        <h3 class="text-lg font-extrabold">Voz (TTS)</h3>
        <p class="text-base text-white/60">
          Motor: <strong>{tts?.engine ?? "…"}</strong> · Voz: <strong>{tts?.voice ?? "…"}</strong>
        </p>
      </div>
    </section>

    <!-- Backup -->
    <section class="card p-6">
      <h2 class="text-2xl font-extrabold">💾 Copia de seguridad</h2>
      <p class="mt-1 text-base text-white/60">Tu progreso, a salvo. Exporta o restaura cuando quieras.</p>
      <div class="mt-4 flex flex-wrap gap-3">
        <button class="pressable rounded-2xl bg-white/10 px-6 py-3 text-lg font-bold hover:bg-white/20" onclick={exportBackup}>
          ⬆️ Exportar
        </button>
        <input bind:this={fileInput} type="file" class="hidden" aria-label="Archivo de copia de seguridad" onchange={onRestoreFile} />
        <button class="pressable rounded-2xl bg-white/10 px-6 py-3 text-lg font-bold hover:bg-white/20" onclick={() => fileInput?.click()}>
          ⬇️ Restaurar
        </button>
      </div>
      {#if backupPath}
        <p class="anim-pop mt-3 rounded-2xl bg-lime-500/10 p-3 font-mono text-base text-lime-500">{backupPath}</p>
      {/if}
    </section>

    {#if credits}
      <section class="card p-6">
        <h2 class="text-2xl font-extrabold">📚 Créditos de los datos</h2>
        <p class="mt-2 text-base text-white/60">{credits}</p>
      </section>
    {/if}

    <!-- Danger zone -->
    <section class="card border-coral-500/40 p-6">
      <h2 class="text-2xl font-extrabold text-coral-500">⚠️ Zona de peligro</h2>
      <p class="mt-1 text-base text-white/60">Borra todo tu progreso. No hay marcha atrás (bueno, sí: la copia de seguridad).</p>
      <button
        class="pressable mt-4 rounded-2xl bg-coral-500 px-6 py-3 text-lg font-extrabold text-white hover:brightness-110"
        onclick={() => (showReset = true)}
      >
        Reiniciar progreso
      </button>
    </section>
  {:else}
    <p class="text-lg text-white/50">Cargando ajustes…</p>
  {/if}

  {#if savedFlash}
    <div class="anim-pop fixed bottom-8 left-1/2 z-50 -translate-x-1/2 rounded-full bg-lime-500 px-8 py-3 text-xl font-black text-night-900 shadow-2xl" role="status">
      ¡Guardado! ✓
    </div>
  {/if}
</div>

<Modal open={showRestore} title="¿Restaurar copia de seguridad?" onclose={() => (showRestore = false)}>
  <p class="text-lg text-white/70">
    Se reemplazará tu progreso actual por el de la copia <strong>{restoreFile?.name}</strong>. ¿Seguro?
  </p>
  <div class="mt-6 flex gap-3">
    <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-bold hover:bg-white/20" onclick={() => (showRestore = false)}>
      Cancelar
    </button>
    <button class="pressable flex-1 rounded-2xl bg-lime-500 py-3 text-lg font-extrabold text-night-900 hover:brightness-110" onclick={doRestore}>
      Restaurar
    </button>
  </div>
</Modal>

<Modal open={showReset} title="¿Reiniciar todo el progreso?" onclose={() => (showReset = false)} danger>
  <p class="text-lg text-white/70">
    Esto borra todas tus palabras aprendidas, XP e insignias. Preferimos que sigas jugando… pero es tu decisión. ¿Confirmas?
  </p>
  <div class="mt-6 flex gap-3">
    <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-bold hover:bg-white/20" onclick={() => (showReset = false)}>
      Mejor no
    </button>
    <button
      class="pressable flex-1 rounded-2xl bg-coral-500 py-3 text-lg font-extrabold text-white hover:brightness-110"
      onclick={() => {
        showReset = false;
        showReset2 = true;
      }}
    >
      Sí, confirmo
    </button>
  </div>
</Modal>

<Modal open={showReset2} title="Última confirmación" onclose={() => (showReset2 = false)} danger>
  <p class="text-lg text-white/70">De verdad, de verdad. No hay vuelta atrás después de esto.</p>
  <div class="mt-6 flex gap-3">
    <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-bold hover:bg-white/20" onclick={() => (showReset2 = false)}>
      ¡Cancelar!
    </button>
    <button class="pressable flex-1 rounded-2xl bg-coral-500 py-3 text-lg font-extrabold text-white hover:brightness-110" onclick={doReset}>
      Borrar todo
    </button>
  </div>
</Modal>

{#if resetDone}
  <div class="fixed inset-0 z-[9500] flex items-center justify-center bg-night-900/90">
    <p class="anim-pop text-3xl font-extrabold text-white/80">Empezando de cero… 🌱</p>
  </div>
{/if}
