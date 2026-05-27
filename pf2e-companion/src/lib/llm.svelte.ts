// Shared LLM state. Mirrors lens.svelte.ts / campaign.svelte.ts shape.
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  llmStatus,
  type LlmStatus,
  type LlmTokenEvent,
} from "$lib/ipc";

export const llmState = $state({
  status: null as LlmStatus | null,
  loaded: false,
  error: null as string | null,
});

export async function refreshLlmStatus() {
  try {
    llmState.status = await llmStatus();
    llmState.error = null;
  } catch (e) {
    llmState.error = String(e);
  } finally {
    llmState.loaded = true;
  }
}

let tokenUnlisten: UnlistenFn | null = null;

/**
 * Subscribe to streaming tokens for a chat session. The handler is invoked
 * for every event whose `session_id` matches the supplied id; events for
 * other sessions are ignored. Returns an unsubscribe fn.
 */
export async function subscribeToSession(
  sessionId: string,
  onEvent: (e: LlmTokenEvent) => void,
): Promise<() => void> {
  if (!tokenUnlisten) {
    // Single global listener; we filter by session id per subscriber.
    // Not strictly necessary, but cheaper than re-attaching per session.
  }
  const unlisten = await listen<LlmTokenEvent>("llm:token", (event) => {
    if (event.payload.session_id === sessionId) {
      onEvent(event.payload);
    }
  });
  return () => unlisten();
}
