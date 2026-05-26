<script lang="ts">
  import { onMount } from "svelte";
  import { search, schemaVersion, type SearchHit } from "$lib/ipc";
  import { lensState } from "$lib/lens.svelte";

  let query = $state("Hosts");
  let typeFilter = $state("");
  let hits = $state<SearchHit[]>([]);
  let version = $state<number | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  const types = [
    { id: "", label: "All types" },
    { id: "deity", label: "Deities" },
    { id: "saint", label: "Saints" },
    { id: "spell", label: "Spells" },
    { id: "creature", label: "Creatures" },
    { id: "feat", label: "Feats" },
    { id: "npc", label: "NPCs" },
    { id: "note", label: "Notes" },
  ];

  let filteredHits = $derived(
    typeFilter ? hits.filter((h) => h.type === typeFilter) : hits,
  );

  async function runSearch() {
    if (!query.trim()) {
      hits = [];
      return;
    }
    loading = true;
    error = null;
    try {
      hits = await search(query, lensState.active);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    try {
      version = await schemaVersion();
    } catch (e) {
      error = `schema probe failed: ${e}`;
    }
    await runSearch();
  });

  $effect(() => {
    void lensState.active;
    runSearch();
  });
</script>

<section class="hero">
  <h1>Search</h1>
  <p class="tagline">PF2e Remaster + Christian Biblical worldview reference</p>
  {#if version !== null}
    <p class="meta">schema v{version}</p>
  {/if}
</section>

<form
  class="search-bar"
  onsubmit={(e) => {
    e.preventDefault();
    runSearch();
  }}
>
  <input
    type="search"
    placeholder="Search canon, rules, names..."
    bind:value={query}
  />
  <button type="submit" disabled={loading}>
    {loading ? "…" : "Search"}
  </button>
</form>

<div class="filters">
  {#each types as t (t.id)}
    <button
      type="button"
      class="chip"
      class:active={typeFilter === t.id}
      onclick={() => (typeFilter = t.id)}
    >
      {t.label}
    </button>
  {/each}
</div>

{#if error}
  <p class="err">{error}</p>
{/if}

<ul class="hits">
  {#each filteredHits as hit (hit.id)}
    <li>
      <header>
        <span class="type">{hit.type}</span>
        <h2>{hit.title}</h2>
        <span class="score">{hit.score.toFixed(2)}</span>
      </header>
      <p class="snippet">{@html hit.snippet}</p>
    </li>
  {:else}
    <li class="empty">
      {loading ? "Searching…" : 'No hits. Try "Hosts", "smoke", or browse the other tabs.'}
    </li>
  {/each}
</ul>

<style>
  .hero h1 {
    margin: 0;
    font-size: 1.4rem;
    font-weight: 600;
  }
  .tagline {
    margin: 0.1rem 0 0;
    color: var(--muted);
    font-size: 0.85rem;
  }
  .meta {
    margin: 0.2rem 0 0;
    color: var(--muted);
    font-size: 0.7rem;
    font-variant-numeric: tabular-nums;
  }

  .search-bar {
    display: flex;
    gap: 0.5rem;
    margin-top: 1rem;
  }
  .search-bar input {
    flex: 1;
    font: inherit;
    padding: 0.6rem 0.75rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
  }
  .search-bar button {
    font: inherit;
    padding: 0.55rem 1rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: color-mix(in srgb, currentColor 10%, transparent);
    color: inherit;
    cursor: pointer;
  }
  .search-bar button[disabled] {
    opacity: 0.6;
    cursor: progress;
  }

  .filters {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
    margin-top: 0.75rem;
  }
  .chip {
    font: inherit;
    font-size: 0.78rem;
    padding: 0.3rem 0.65rem;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: var(--muted);
    cursor: pointer;
  }
  .chip.active {
    color: inherit;
    background: color-mix(in srgb, currentColor 12%, transparent);
    border-color: color-mix(in srgb, currentColor 30%, transparent);
  }

  .err {
    margin-top: 1rem;
    color: #c33;
    font-size: 0.85rem;
    padding: 0.5rem 0.75rem;
    border: 1px solid #c33;
    border-radius: 6px;
    background: rgba(204, 51, 51, 0.05);
  }

  .hits {
    list-style: none;
    padding: 0;
    margin: 1rem 0 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .hits li {
    padding: 0.75rem 1rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
  }
  .hits li.empty {
    color: var(--muted);
    text-align: center;
  }
  .hits header {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }
  .hits h2 {
    margin: 0;
    font-size: 1rem;
    flex: 1;
  }
  .type {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .score {
    font-size: 0.7rem;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .snippet {
    margin: 0.4rem 0 0;
    font-size: 0.9rem;
    line-height: 1.4;
  }
  .snippet :global(mark) {
    background: rgba(255, 215, 0, 0.35);
    padding: 0 0.1em;
    border-radius: 2px;
    color: inherit;
  }
</style>
