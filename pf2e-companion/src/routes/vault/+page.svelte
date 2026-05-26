<script lang="ts">
  import { onMount } from "svelte";
  import {
    createCampaign,
    listEntities,
    type EntitySummary,
  } from "$lib/ipc";
  import { campaignState, refreshCampaigns } from "$lib/campaign.svelte";
  import { lensState } from "$lib/lens.svelte";

  let entities = $state<EntitySummary[]>([]);
  let typeFilter = $state("");
  let loading = $state(false);
  let error = $state<string | null>(null);

  let newCampaignName = $state("");
  let newCampaignLens = $state<string>("lewisian");
  let creating = $state(false);

  const types = [
    { id: "", label: "All" },
    { id: "npc", label: "NPC" },
    { id: "location", label: "Location" },
    { id: "faction", label: "Faction" },
    { id: "quest", label: "Quest" },
    { id: "session", label: "Session" },
    { id: "note", label: "Note" },
    { id: "creature", label: "Creature" },
  ];

  async function refreshEntities() {
    loading = true;
    error = null;
    try {
      entities = await listEntities(
        typeFilter || undefined,
        lensState.active,
      );
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function handleCreateCampaign(e: Event) {
    e.preventDefault();
    if (!newCampaignName.trim()) return;
    creating = true;
    error = null;
    try {
      const c = await createCampaign(newCampaignName, newCampaignLens || null);
      await refreshCampaigns(true);
      campaignState.active = c.id;
      newCampaignName = "";
    } catch (e) {
      error = String(e);
    } finally {
      creating = false;
    }
  }

  onMount(async () => {
    await refreshCampaigns(true);
    await refreshEntities();
  });

  $effect(() => {
    void [typeFilter, lensState.active, campaignState.active];
    refreshEntities();
  });

  let activeCampaignEntities = $derived(
    entities.filter((e) => {
      // Best-effort: vault entities use ids of the form `<campaign>:<type>/<slug>`.
      if (!campaignState.active) return true;
      return e.id.startsWith(`${campaignState.active}:`);
    }),
  );

  const newEntityHref = $derived.by(() => {
    if (!campaignState.active) return "/vault/new";
    return `/vault/new?campaign=${encodeURIComponent(campaignState.active)}`;
  });
</script>

<section class="hero">
  <h1>Vault</h1>
  <p class="tagline">
    Your worldbuilding files live in <code>~/Library/Application Support/io.github.gkrause.pf2e-companion/vault/</code>
    on macOS (equivalent on other platforms). Plain markdown + frontmatter.
  </p>
</section>

<section class="campaigns">
  <header>
    <h2>Campaigns</h2>
    <a class="btn" href={newEntityHref}>+ New entity</a>
  </header>
  {#if campaignState.campaigns.length === 0}
    <p class="muted">No campaigns yet.</p>
  {:else}
    <ul class="camp-list">
      {#each campaignState.campaigns as c (c.id)}
        <li>
          <button
            type="button"
            class="camp"
            class:active={campaignState.active === c.id}
            onclick={() => (campaignState.active = c.id)}
          >
            <span class="name">{c.name}</span>
            <span class="count">{c.entity_count}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <form class="new-campaign" onsubmit={handleCreateCampaign}>
    <input
      type="text"
      placeholder="New campaign name"
      bind:value={newCampaignName}
    />
    <select bind:value={newCampaignLens}>
      {#each lensState.manifests as l (l.id)}
        <option value={l.id}>{l.label}</option>
      {/each}
    </select>
    <button type="submit" disabled={creating || !newCampaignName.trim()}>
      {creating ? "…" : "Create"}
    </button>
  </form>
</section>

<section class="entities">
  <header>
    <h2>
      Entities {campaignState.active ? `· ${campaignState.active}` : ""}
    </h2>
  </header>

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
    {#each activeCampaignEntities as e (e.id)}
      <li>
        <a class="hit" href={`/vault/${encodeURIComponent(e.id)}`}>
          <span class="type">{e.type}</span>
          <span class="title">{e.title}</span>
          {#if e.lens}<span class="lens">{e.lens}</span>{/if}
        </a>
      </li>
    {:else}
      <li class="empty">
        {loading ? "Loading…" : "No entities in this filter. Create one above."}
      </li>
    {/each}
  </ul>
</section>

<style>
  .hero h1 {
    margin: 0;
    font-size: 1.4rem;
    font-weight: 600;
  }
  .tagline {
    margin: 0.1rem 0 0;
    color: var(--muted);
    font-size: 0.83rem;
  }
  .tagline code {
    background: var(--bg-soft);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
    font-size: 0.78rem;
  }

  section {
    margin-top: 1.5rem;
  }
  section header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 0.5rem;
  }
  section h2 {
    margin: 0;
    font-size: 0.95rem;
    color: var(--muted);
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    font-size: 0.75rem;
  }
  .btn {
    font: inherit;
    font-size: 0.8rem;
    padding: 0.35rem 0.75rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: color-mix(in srgb, currentColor 10%, transparent);
    color: inherit;
    cursor: pointer;
  }
  .btn:hover {
    background: color-mix(in srgb, currentColor 15%, transparent);
  }

  .camp-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr));
    gap: 0.4rem;
  }
  .camp {
    width: 100%;
    text-align: left;
    font: inherit;
    padding: 0.55rem 0.75rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
    cursor: pointer;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .camp.active {
    background: color-mix(in srgb, currentColor 12%, transparent);
    border-color: color-mix(in srgb, currentColor 30%, transparent);
  }
  .camp .name {
    font-weight: 500;
  }
  .camp .count {
    color: var(--muted);
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
  }

  .new-campaign {
    margin-top: 0.6rem;
    display: flex;
    gap: 0.4rem;
  }
  .new-campaign input {
    flex: 1;
    font: inherit;
    padding: 0.5rem 0.7rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
  }
  .new-campaign select {
    font: inherit;
    padding: 0.45rem 0.55rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
  }
  .new-campaign button {
    font: inherit;
    padding: 0.5rem 0.85rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: color-mix(in srgb, currentColor 10%, transparent);
    color: inherit;
    cursor: pointer;
  }

  .filters {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
    margin-bottom: 0.6rem;
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

  .hits {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .hits li {
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    overflow: hidden;
  }
  .hits .empty {
    padding: 0.75rem 1rem;
    color: var(--muted);
    text-align: center;
  }
  .hit {
    display: flex;
    gap: 0.6rem;
    align-items: baseline;
    padding: 0.6rem 0.85rem;
    color: inherit;
    text-decoration: none;
  }
  .hit:hover {
    background: color-mix(in srgb, currentColor 4%, transparent);
  }
  .type {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    min-width: 4rem;
  }
  .title {
    flex: 1;
    font-weight: 500;
  }
  .lens {
    font-size: 0.7rem;
    color: var(--muted);
    border: 1px solid var(--line);
    padding: 0.05rem 0.4rem;
    border-radius: 999px;
  }

  .err {
    color: #c33;
    margin: 0.5rem 0;
    padding: 0.4rem 0.7rem;
    border: 1px solid #c33;
    border-radius: 6px;
    background: rgba(204, 51, 51, 0.05);
  }
  .muted {
    color: var(--muted);
  }
</style>
