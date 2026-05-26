// Shared lens state. SvelteKit + Svelte 5 runes; lives across routes.
import { listLenses, type LensManifest } from "$lib/ipc";

export const lensState = $state({
  active: "lewisian" as string,
  manifests: [] as LensManifest[],
  loaded: false,
});

export async function ensureLensesLoaded() {
  if (lensState.loaded) return;
  try {
    lensState.manifests = await listLenses();
  } catch {
    lensState.manifests = [];
  }
  lensState.loaded = true;
}
