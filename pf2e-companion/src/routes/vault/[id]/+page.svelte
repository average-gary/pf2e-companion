<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import EntityEditor from "$lib/EntityEditor.svelte";
  import { getEntity, type EntityDetail } from "$lib/ipc";
  import { refreshCampaigns } from "$lib/campaign.svelte";

  let entity = $state<EntityDetail | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load(id: string) {
    loading = true;
    error = null;
    entity = null;
    try {
      entity = await getEntity(id);
      if (!entity) error = `No entity with id "${id}".`;
      else if (entity.source !== "vault") {
        error = `Cannot edit reference content. ${entity.title} is bundled.`;
        entity = null;
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => refreshCampaigns(true));

  $effect(() => {
    void load(decodeURIComponent(page.params.id ?? ""));
  });
</script>

{#if loading}
  <p class="muted">Loading…</p>
{:else if error}
  <p class="err">{error}</p>
{:else if entity}
  <EntityEditor mode="edit" {entity} />
{/if}

<style>
  .err {
    color: #c33;
    padding: 0.5rem 0.75rem;
    border: 1px solid #c33;
    border-radius: 6px;
    background: rgba(204, 51, 51, 0.05);
    margin-top: 1rem;
  }
  .muted {
    color: var(--muted);
  }
</style>
