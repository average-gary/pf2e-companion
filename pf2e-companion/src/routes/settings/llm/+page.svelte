<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import {
    llmConfigure,
    llmClearConfig,
    type LlmProviderKind,
  } from "$lib/ipc";
  import { llmState, refreshLlmStatus } from "$lib/llm.svelte";

  let provider = $state<LlmProviderKind>("ollama");
  let model = $state("qwen3:4b");
  let baseUrl = $state("http://localhost:11434");
  let apiKey = $state("");
  let saving = $state(false);
  let savedAt = $state<number | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    await refreshLlmStatus();
    if (llmState.status?.configured) {
      provider = (llmState.status.provider ?? "ollama") as LlmProviderKind;
      model = llmState.status.model ?? defaultModelFor(provider);
    }
  });

  function defaultModelFor(p: LlmProviderKind): string {
    return p === "anthropic" ? "claude-sonnet-4-6" : "qwen3:4b";
  }

  function defaultBaseUrlFor(p: LlmProviderKind): string {
    return p === "anthropic" ? "https://api.anthropic.com" : "http://localhost:11434";
  }

  $effect(() => {
    // When provider changes, swap defaults if the user hasn't customized yet.
    void provider;
  });

  async function save(e: Event) {
    e.preventDefault();
    saving = true;
    error = null;
    try {
      await llmConfigure(
        {
          provider,
          model,
          base_url: provider === "ollama" ? baseUrl : null,
        },
        provider === "anthropic" && apiKey ? apiKey : null,
      );
      apiKey = "";
      savedAt = Date.now();
      await refreshLlmStatus();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function clear() {
    if (!confirm("Disable the LLM? Any stored API key will be removed.")) {
      return;
    }
    try {
      await llmClearConfig();
      await refreshLlmStatus();
      savedAt = null;
    } catch (e) {
      error = String(e);
    }
  }
</script>

<header class="hero">
  <button
    type="button"
    class="back"
    onclick={() => (history.length > 1 ? history.back() : goto("/"))}
  >
    ← Back
  </button>
  <h1>LLM settings</h1>
  <p class="tagline">
    Off by default. The companion runs fully without it. Enable only if you
    want chat-style assistance grounded in the bundled content packs.
  </p>
</header>

<section class="disclaimer">
  <h2>Before you enable</h2>
  <p>
    The PF2e community is generally fine with prose-drafting AI but skeptical
    of rules-touching AI. <strong>Always verify rules-touching outputs</strong>
    against the canonical spell / monster / encounter math before bringing them
    to the table.
  </p>
  <p>
    Local models (Ollama) keep everything on your machine. Cloud models
    (Anthropic) send your prompts and the active lens's content out to the
    provider — review their privacy and data-retention policies before
    configuring.
  </p>
</section>

<form class="form" onsubmit={save}>
  <fieldset>
    <legend>Provider</legend>
    <label class="radio">
      <input
        type="radio"
        name="provider"
        value="ollama"
        bind:group={provider}
        onchange={() => {
          model = defaultModelFor("ollama");
          baseUrl = defaultBaseUrlFor("ollama");
        }}
      />
      <span class="r-title">Ollama (local, recommended)</span>
      <span class="r-sub">No API key. Runs on <code>localhost:11434</code> by default. Install <a href="https://ollama.com" target="_blank" rel="noopener noreferrer">Ollama</a> and pull a model first.</span>
    </label>
    <label class="radio">
      <input
        type="radio"
        name="provider"
        value="anthropic"
        bind:group={provider}
        onchange={() => {
          model = defaultModelFor("anthropic");
        }}
      />
      <span class="r-title">Anthropic (cloud)</span>
      <span class="r-sub">Requires an API key. Stored in your OS keychain. Anthropic does <em>not</em> provide embeddings — RAG indexing falls back to a separate Ollama embed model.</span>
    </label>
  </fieldset>

  <label>
    <span class="lbl">Model</span>
    <input type="text" bind:value={model} required />
    <span class="hint">
      {#if provider === "ollama"}
        Examples: <code>qwen3:4b</code>, <code>qwen3:14b</code>, <code>llama3.2:3b</code>.
      {:else}
        Examples: <code>claude-sonnet-4-6</code>, <code>claude-opus-4-7</code>, <code>claude-haiku-4-5-20251001</code>.
      {/if}
    </span>
  </label>

  {#if provider === "ollama"}
    <label>
      <span class="lbl">Base URL</span>
      <input type="text" bind:value={baseUrl} required />
    </label>
  {:else}
    <label>
      <span class="lbl">API key</span>
      <input
        type="password"
        bind:value={apiKey}
        placeholder={llmState.status?.key_present
          ? "(stored — leave blank to reuse)"
          : "sk-ant-…"}
        autocomplete="off"
      />
      <span class="hint">
        Stored in the OS keychain (macOS Keychain / Windows Credential
        Manager / libsecret). Never written to disk in plaintext.
      </span>
    </label>
  {/if}

  {#if error}
    <p class="err">{error}</p>
  {/if}

  <footer>
    <button type="submit" class="primary" disabled={saving}>
      {saving ? "Saving…" : "Save"}
    </button>
    {#if llmState.status?.configured}
      <button type="button" class="danger" onclick={clear}>
        Disable LLM
      </button>
    {/if}
    {#if savedAt}
      <span class="ok">Saved.</span>
    {/if}
  </footer>
</form>

{#if llmState.status?.configured}
  <p class="status">
    Currently configured: <strong>{llmState.status.provider}</strong>
    · model <code>{llmState.status.model}</code>.
    <a href="/chat">Open chat →</a>
  </p>
{/if}

<style>
  .hero {
    margin-bottom: 1rem;
  }
  .back {
    align-self: flex-start;
    font: inherit;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0.25rem 0;
    margin-bottom: 0.4rem;
    font-size: 0.85rem;
  }
  h1 {
    margin: 0.1rem 0 0;
    font-size: 1.4rem;
    font-weight: 600;
  }
  .tagline {
    margin: 0.2rem 0 0;
    color: var(--muted);
    font-size: 0.85rem;
  }

  .disclaimer {
    margin: 1rem 0 1.5rem;
    padding: 0.85rem 1rem;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    font-size: 0.85rem;
    line-height: 1.5;
  }
  .disclaimer h2 {
    margin: 0 0 0.4rem;
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .disclaimer p {
    margin: 0.5rem 0;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
  }

  fieldset {
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 0.6rem 0.85rem;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  legend {
    padding: 0 0.4rem;
    font-size: 0.78rem;
    color: var(--muted);
  }
  .radio {
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: 0.5rem;
    row-gap: 0.15rem;
    align-items: baseline;
    cursor: pointer;
  }
  .radio input {
    grid-column: 1;
    grid-row: 1 / 3;
    align-self: center;
  }
  .r-title {
    font-weight: 500;
  }
  .r-sub {
    color: var(--muted);
    font-size: 0.78rem;
    line-height: 1.4;
  }
  .r-sub a {
    color: inherit;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, currentColor 30%, transparent);
  }

  label:not(.radio) {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .lbl {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .hint {
    color: var(--muted);
    font-size: 0.75rem;
  }
  input[type="text"],
  input[type="password"] {
    font: inherit;
    padding: 0.5rem 0.7rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
  }
  code {
    background: var(--bg-soft);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
    font-size: 0.85em;
  }

  footer {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.5rem;
    padding-top: 0.6rem;
    border-top: 1px solid var(--line);
  }
  footer button {
    font: inherit;
    padding: 0.55rem 1rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
    cursor: pointer;
  }
  footer .primary {
    background: color-mix(in srgb, currentColor 12%, transparent);
    border-color: color-mix(in srgb, currentColor 30%, transparent);
    font-weight: 500;
  }
  footer .danger {
    color: #c33;
    margin-left: auto;
    border-color: color-mix(in srgb, #c33 30%, var(--line));
  }
  footer button[disabled] {
    opacity: 0.6;
    cursor: progress;
  }
  .ok {
    color: var(--muted);
    font-size: 0.85rem;
  }

  .err {
    color: #c33;
    padding: 0.5rem 0.75rem;
    border: 1px solid #c33;
    border-radius: 6px;
    background: rgba(204, 51, 51, 0.05);
  }

  .status {
    margin-top: 1.2rem;
    padding: 0.6rem 0.85rem;
    border-radius: 8px;
    background: var(--bg-soft);
    border: 1px solid var(--line);
    font-size: 0.85rem;
  }
  .status a {
    color: inherit;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, currentColor 30%, transparent);
    margin-left: 0.4rem;
  }
</style>
