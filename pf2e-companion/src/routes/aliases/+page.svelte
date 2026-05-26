<script lang="ts">
  import { onMount } from "svelte";
  import { lookupAlias, type AliasHit } from "$lib/ipc";

  let query = $state("Magic Missile");
  let hits = $state<AliasHit[]>([]);
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
      hits = await lookupAlias(query);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(run);
</script>

<section class="hero">
  <h1>Remaster aliases</h1>
  <p class="tagline">
    Legacy ↔ Remaster name lookup. Sourced from
    <code>pf2e-remaster-name-mapping.md</code>.
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
    placeholder="e.g. Magic Missile, Aasimar, Mithral, Force Barrage"
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
  {#each hits as h, i (`${h.legacy_name}->${h.remaster_name}-${i}`)}
    <li>
      <div class="rename">
        <span class="legacy">{h.legacy_name}</span>
        <span class="arrow">→</span>
        <span class="remaster">{h.remaster_name}</span>
      </div>
      <div class="meta">
        <span class="cat">{h.category}</span>
        {#if h.notes}<span class="note">{h.notes}</span>{/if}
      </div>
    </li>
  {:else}
    <li class="empty">
      {loading
        ? "Looking up…"
        : "Type a legacy or Remaster name above. Try Magic Missile, Aasimar, or Mithral."}
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
    padding: 0.75rem 1rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
  }
  .hits li.empty {
    color: var(--muted);
    text-align: center;
  }

  .rename {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .legacy {
    color: var(--muted);
    text-decoration: line-through;
    font-size: 0.95rem;
  }
  .arrow {
    color: var(--muted);
  }
  .remaster {
    font-weight: 600;
    font-size: 1rem;
  }

  .meta {
    margin-top: 0.3rem;
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    font-size: 0.78rem;
    color: var(--muted);
  }
  .cat {
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0.05rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 999px;
  }
</style>
