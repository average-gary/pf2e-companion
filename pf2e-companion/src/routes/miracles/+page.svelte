<script lang="ts">
  import { onMount } from "svelte";
  import { lookupMiracle, type MiracleHit } from "$lib/ipc";

  let query = $state("Mt 14:25");
  let hits = $state<MiracleHit[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function run() {
    if (!query.trim()) {
      hits = [];
      return;
    }
    loading = true;
    error = null;
    try {
      hits = await lookupMiracle(query);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(run);
</script>

<section class="hero">
  <h1>Miracle ↔ spell map</h1>
  <p class="tagline">
    Look up a Bible miracle, reference, or book and see the suggested PF2e
    Remaster spell. Sourced from
    <code>biblical-miracle-to-pf2e-spell-map.md</code>.
  </p>
</section>

<form
  class="search-bar"
  onsubmit={(e) => {
    e.preventDefault();
    run();
  }}
>
  <input
    type="search"
    placeholder="e.g. Mt 14:25, Elijah, 1 Kgs 18:38, Gospels"
    bind:value={query}
  />
  <button type="submit" disabled={loading}>
    {loading ? "…" : "Look up"}
  </button>
</form>

{#if error}
  <p class="err">{error}</p>
{/if}

<ul class="hits">
  {#each hits as h (h.miracle)}
    <li>
      <header>
        <h2>{h.miracle}</h2>
        <span class="ref">{h.reference}</span>
      </header>
      <div class="spell">
        <span class="lbl">→</span>
        <span class="name">{h.spell_name}</span>
      </div>
      <div class="meta">
        <span class="book">{h.book}</span>
        {#if h.tradition}<span class="trad">{h.tradition}</span>{/if}
        {#if h.sanctification}
          <span class="sanct" data-s={h.sanctification.toLowerCase()}>
            {h.sanctification}
          </span>
        {/if}
      </div>
      {#if h.notes}<p class="note">{h.notes}</p>{/if}
    </li>
  {:else}
    <li class="empty">
      {loading
        ? "Looking up…"
        : 'Try "Mt 14:25", "Elijah", "Gospels", or "1 Kgs 18:38".'}
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
  .tagline code {
    font-size: 0.78rem;
    background: var(--bg-soft);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
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

  .err {
    color: #c33;
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
    padding: 0.85rem 1rem;
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
    gap: 0.6rem;
    flex-wrap: wrap;
  }
  .hits h2 {
    margin: 0;
    font-size: 1rem;
    flex: 1;
  }
  .ref {
    font-size: 0.78rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .spell {
    margin-top: 0.4rem;
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
  }
  .spell .lbl {
    color: var(--muted);
  }
  .spell .name {
    font-weight: 600;
  }
  .meta {
    margin-top: 0.4rem;
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    font-size: 0.74rem;
    color: var(--muted);
  }
  .meta span {
    padding: 0.05rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 999px;
  }
  .sanct[data-s="holy"] {
    color: hsl(45 80% 45%);
    border-color: hsl(45 80% 45% / 0.4);
  }
  .sanct[data-s="unholy"] {
    color: hsl(280 50% 50%);
    border-color: hsl(280 50% 50% / 0.4);
  }
  .note {
    margin: 0.5rem 0 0;
    font-size: 0.85rem;
    line-height: 1.4;
    color: color-mix(in srgb, currentColor 80%, transparent);
  }
</style>
