// Shared campaign state. Mirrors lens.svelte.ts in shape.
import { listCampaigns, type Campaign } from "$lib/ipc";

export const campaignState = $state({
  active: null as string | null,
  campaigns: [] as Campaign[],
  loaded: false,
  loading: false,
});

export async function refreshCampaigns(force = false) {
  if (campaignState.loaded && !force) return;
  campaignState.loading = true;
  try {
    campaignState.campaigns = await listCampaigns();
    if (campaignState.active === null && campaignState.campaigns.length > 0) {
      campaignState.active = campaignState.campaigns[0].id;
    }
  } catch {
    campaignState.campaigns = [];
  } finally {
    campaignState.loaded = true;
    campaignState.loading = false;
  }
}
