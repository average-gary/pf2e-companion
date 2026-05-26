<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import EntityEditor from "$lib/EntityEditor.svelte";
  import { refreshCampaigns } from "$lib/campaign.svelte";

  onMount(() => refreshCampaigns(true));

  let initialCampaign = $derived(page.url.searchParams.get("campaign"));
</script>

<EntityEditor mode="new" {initialCampaign} />
