<script lang="ts">
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { api } from "$lib/api";
  import { setSfxEnabled } from "$lib/sfx";
  import Modal from "$lib/components/Modal.svelte";
  import type { BackupFolder, BackupInfo, BackupKind, Settings, TtsInfo } from "$lib/contract";

  const KIND_LABEL: Record<BackupKind, string> = {
    manual: "Exportada",
    auto: "Automática",
    "before-reset": "Antes de reiniciar",
    "before-restore": "Antes de restaurar",
  };

  let settings = $state<Settings | null>(null);
  let tts = $state<TtsInfo | null>(null);
  let credits = $state("");
  let folder = $state<BackupFolder | null>(null);
  let showAllBackups = $state(false);
  let backupPath = $state<string | null>(null);
  let savedFlash = $state(false);
  let errorMsg = $state<string | null>(null);
  let restoreFrom = $state<{ label: string; run: () => Promise<void> } | null>(null);
  let showReset = $state(false);
  let showReset2 = $state(false);
  let overlay = $state<string | null>(null);
  let fileInput: HTMLInputElement | undefined = $state();

  const shownBackups = $derived(folder ? (showAllBackups ? folder.backups : folder.backups.slice(0, 5)) : []);

  $effect(() => {
    void (async () => {
      settings = await api.getSettings();
      tts = await api.ttsInfo();
      credits = await api.contentCredits();
      folder = await api.backupFolder();
    })();
  });

  function showError(e: unknown) {
    errorMsg = String(e);
    setTimeout(() => (errorMsg = null), 6000);
  }

  function when(b: BackupInfo): string {
    return new Date(b.createdAt * 1000).toLocaleString("es-ES", { dateStyle: "medium", timeStyle: "short" });
  }

  async function save() {
    if (!settings) return;
    settings = await api.setSettings({ ...settings });
    setSfxEnabled(settings.soundEnabled);
    savedFlash = true;
    setTimeout(() => (savedFlash = false), 1500);
  }

  async function chooseFolder() {
    const dir = await pickFolder({ directory: true, defaultPath: folder?.dir, title: "Carpeta para las copias de Tilde" });
    if (typeof dir !== "string") return;
    try {
      folder = await api.setBackupFolder(dir);
    } catch (e) {
      showError(e);
    }
  }

  async function useDefaultFolder() {
    folder = await api.setBackupFolder(null);
  }

  async function exportBackup() {
    try {
      backupPath = await api.exportBackup();
      folder = await api.backupFolder();
    } catch (e) {
      showError(e);
    }
  }

  function onRestoreFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    restoreFrom = {
      label: file.name,
      run: async () => api.importBackup(new Uint8Array(await file.arrayBuffer())),
    };
  }

  async function doRestore() {
    const pending = restoreFrom;
    restoreFrom = null;
    if (!pending) return;
    try {
      await pending.run();
    } catch (e) {
      showError(e);
      return;
    }
    overlay = "Progreso restaurado ✓";
    setTimeout(() => (location.href = "/"), 1400);
  }

  async function doReset() {
    showReset2 = false;
    try {
      await api.resetProgress();
    } catch (e) {
      showError(e);
      return;
    }
    overlay = "Empezando de cero… 🌱";
    setTimeout(() => (location.href = "/"), 1600);
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
      <h2 class="text-2xl font-extrabold">💾 Copias de seguridad</h2>
      <p class="mt-1 text-base text-white/60">
        Tilde guarda una copia sola al terminar cada sesión (se quedan las 10 últimas) y otra antes de restaurar o reiniciar.
      </p>

      {#if folder}
        <div class="mt-4 rounded-2xl bg-white/5 p-4">
          <p class="text-sm font-bold uppercase tracking-wide text-white/40">Carpeta</p>
          <p class="mt-1 break-all font-mono text-base">{folder.dir}</p>
          <p class="mt-2 text-sm text-white/50">
            ¿Tu progreso en todos tus equipos? Elige una carpeta sincronizada (Syncthing, Nextcloud, Dropbox…).
          </p>
          <div class="mt-3 flex flex-wrap gap-2">
            <button class="pressable rounded-xl bg-white/10 px-4 py-2 font-bold hover:bg-white/20" onclick={chooseFolder}>
              📂 Cambiar carpeta
            </button>
            <button
              class="pressable rounded-xl bg-white/10 px-4 py-2 font-bold hover:bg-white/20"
              onclick={() => void revealItemInDir(folder!.dir).catch(showError)}
            >
              Mostrar
            </button>
            {#if !folder.isDefault}
              <button class="pressable rounded-xl px-4 py-2 font-bold text-white/60 hover:bg-white/10" onclick={useDefaultFolder}>
                Volver a la predeterminada
              </button>
            {/if}
          </div>
        </div>
      {/if}

      <div class="mt-4 flex flex-wrap gap-3">
        <button class="pressable rounded-2xl bg-white/10 px-6 py-3 text-lg font-bold hover:bg-white/20" onclick={exportBackup}>
          ⬆️ Exportar ahora
        </button>
        <input bind:this={fileInput} type="file" accept=".zip" class="hidden" aria-label="Archivo de copia de seguridad" onchange={onRestoreFile} />
        <button class="pressable rounded-2xl bg-white/10 px-6 py-3 text-lg font-bold hover:bg-white/20" onclick={() => fileInput?.click()}>
          ⬇️ Restaurar desde archivo…
        </button>
      </div>
      {#if backupPath}
        <p class="anim-pop mt-3 break-all rounded-2xl bg-lime-500/10 p-3 font-mono text-base text-lime-500">{backupPath}</p>
      {/if}

      {#if folder && folder.backups.length > 0}
        <h3 class="mt-6 text-lg font-extrabold">Copias en la carpeta</h3>
        <ul class="mt-2 flex flex-col gap-2">
          {#each shownBackups as b (b.path)}
            <li class="flex items-center gap-3 rounded-2xl bg-white/5 px-4 py-2.5">
              <span class="font-bold">{when(b)}</span>
              <span class="rounded-full bg-white/10 px-2.5 py-0.5 text-sm text-white/60">{KIND_LABEL[b.kind] ?? b.kind}</span>
              <button
                class="pressable ml-auto rounded-xl px-3 py-1.5 font-bold text-grape-400 hover:bg-white/10"
                onclick={() => (restoreFrom = { label: `${KIND_LABEL[b.kind] ?? b.kind} · ${when(b)}`, run: () => api.restoreBackup(b.path) })}
              >
                Restaurar
              </button>
            </li>
          {/each}
        </ul>
        {#if folder.backups.length > 5}
          <button class="mt-2 text-base font-bold text-white/50 hover:text-white/80" onclick={() => (showAllBackups = !showAllBackups)}>
            {showAllBackups ? "Ver menos" : `Ver todas (${folder.backups.length})`}
          </button>
        {/if}
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
      <p class="mt-1 text-base text-white/60">Borra todo tu progreso. Antes guardamos una copia en tu carpeta, por si cambias de idea.</p>
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

  {#if errorMsg}
    <div class="anim-pop fixed bottom-8 left-1/2 z-50 max-w-xl -translate-x-1/2 rounded-3xl bg-coral-500 px-8 py-3 text-lg font-bold text-white shadow-2xl" role="alert">
      {errorMsg}
    </div>
  {/if}

  {#if savedFlash}
    <div class="anim-pop fixed bottom-8 left-1/2 z-50 -translate-x-1/2 rounded-full bg-lime-500 px-8 py-3 text-xl font-black text-night-900 shadow-2xl" role="status">
      ¡Guardado! ✓
    </div>
  {/if}
</div>

<Modal open={restoreFrom !== null} title="¿Restaurar copia de seguridad?" onclose={() => (restoreFrom = null)}>
  <p class="text-lg text-white/70">
    Se reemplazará tu progreso actual por el de <strong>{restoreFrom?.label}</strong>. Antes guardaremos una copia de lo que tienes ahora, por si acaso.
  </p>
  <div class="mt-6 flex gap-3">
    <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-bold hover:bg-white/20" onclick={() => (restoreFrom = null)}>
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
  <p class="text-lg text-white/70">De verdad, de verdad. Solo podrás volver atrás restaurando la copia «Antes de reiniciar».</p>
  <div class="mt-6 flex gap-3">
    <button class="pressable flex-1 rounded-2xl bg-white/10 py-3 text-lg font-bold hover:bg-white/20" onclick={() => (showReset2 = false)}>
      ¡Cancelar!
    </button>
    <button class="pressable flex-1 rounded-2xl bg-coral-500 py-3 text-lg font-extrabold text-white hover:brightness-110" onclick={doReset}>
      Borrar todo
    </button>
  </div>
</Modal>

{#if overlay}
  <div class="fixed inset-0 z-[9500] flex items-center justify-center bg-night-900/90">
    <p class="anim-pop text-3xl font-extrabold text-white/80">{overlay}</p>
  </div>
{/if}
