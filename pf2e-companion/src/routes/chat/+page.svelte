<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { llmChat, type ChatMessage } from "$lib/ipc";
  import {
    llmState,
    refreshLlmStatus,
    subscribeToSession,
  } from "$lib/llm.svelte";
  import { lensState } from "$lib/lens.svelte";

  /** Tool-call trace bound to the assistant turn that issued it. */
  type ToolTrace = {
    id: string;
    name: string;
    input: unknown;
    /** undefined while the call is still running. */
    result?: unknown;
    error?: boolean;
  };

  /** UI message shape — extends the wire ChatMessage with a per-turn trace. */
  type UiMessage = ChatMessage & { traces?: ToolTrace[] };

  let messages = $state<UiMessage[]>([]);
  let draft = $state("");
  let streaming = $state(false);
  let error = $state<string | null>(null);
  let lastUsage = $state<
    | {
        input?: number;
        output?: number;
        cacheRead?: number;
        cacheCreated?: number;
        iterations?: number;
      }
    | null
  >(null);
  let showContext = $state(true);
  let unlisten: (() => void) | null = null;

  onMount(refreshLlmStatus);

  onDestroy(() => {
    unlisten?.();
  });

  function buildSystem(): string {
    return [
      `You are a helpful PF2e Remaster + Christian Biblical worldview reference assistant.`,
      `The user's active denominational lens is: ${lensState.active}.`,
      ``,
      `Tools available:`,
      `- xp_budget(party_size, difficulty) — encounter XP budget`,
      `- search(query, lens?) — hybrid FTS + vector search over the bundled lens content`,
      `- lookup_alias(name) — legacy → Remaster name table`,
      `- lookup_miracle(query) — biblical miracle → spell map`,
      `- validate_statblock(statblock) — sanctification + license-provenance checks`,
      `Use them when they help — for example, call search() before claiming a doctrine appears in a lens, and xp_budget() before sizing an encounter.`,
      ``,
      `Important constraints:`,
      `- Do NOT require real-world prayer or piety as a mechanical input. The Jesus Prayer triggers on a spell action, not on the player saying it aloud.`,
      `- For rules-touching answers, suggest verifying against canonical PF2e Remaster sources.`,
      `- When citing the bundled content, prefer the user's active lens.`,
    ].join("\n");
  }

  async function send(e: Event) {
    e.preventDefault();
    if (!draft.trim() || streaming) return;
    if (!llmState.status?.configured) {
      error = "LLM is not configured. Visit /settings/llm first.";
      return;
    }

    const userMessage: UiMessage = { role: "user", content: draft.trim() };
    messages = [...messages, userMessage];
    draft = "";
    error = null;
    lastUsage = null;
    streaming = true;

    // Open the assistant slot. Strip the UI-only `traces` field before
    // sending the conversation upstream.
    const assistantIdx = messages.length;
    messages = [...messages, { role: "assistant", content: "", traces: [] }];

    try {
      const wire: ChatMessage[] = messages
        .slice(0, -1)
        .map(({ role, content }) => ({ role, content }));
      const sessionId = await llmChat(wire, {
        system: buildSystem(),
        temperature: 0.6,
        maxTokens: 1024,
        cacheSystem: llmState.status?.provider === "anthropic",
      });

      unlisten?.();
      unlisten = await subscribeToSession(sessionId, (ev) => {
        switch (ev.kind) {
          case "token":
            messages = messages.map((m, i) =>
              i === assistantIdx ? { ...m, content: m.content + ev.token } : m,
            );
            break;
          case "tool_start":
            messages = messages.map((m, i) =>
              i === assistantIdx
                ? {
                    ...m,
                    traces: [
                      ...(m.traces ?? []),
                      { id: ev.id, name: ev.name, input: ev.input },
                    ],
                  }
                : m,
            );
            break;
          case "tool_result":
            messages = messages.map((m, i) =>
              i === assistantIdx
                ? {
                    ...m,
                    traces: (m.traces ?? []).map((t) =>
                      t.id === ev.id
                        ? { ...t, result: ev.result, error: ev.error }
                        : t,
                    ),
                  }
                : m,
            );
            break;
          case "done":
            streaming = false;
            lastUsage = {
              input: ev.input_tokens ?? undefined,
              output: ev.output_tokens ?? undefined,
              cacheRead: ev.cache_read_input_tokens ?? undefined,
              cacheCreated: ev.cache_creation_input_tokens ?? undefined,
              iterations: ev.iterations,
            };
            unlisten?.();
            unlisten = null;
            break;
          case "error":
            error = ev.message;
            streaming = false;
            unlisten?.();
            unlisten = null;
            break;
        }
      });
    } catch (e) {
      error = String(e);
      streaming = false;
      // Drop the empty assistant slot if invocation failed before stream began.
      messages = messages.slice(0, -1);
    }
  }

  function reset() {
    if (streaming) return;
    messages = [];
    error = null;
    lastUsage = null;
  }

  function summarizeInput(v: unknown): string {
    try {
      const s = JSON.stringify(v);
      return s.length > 80 ? s.slice(0, 77) + "…" : s;
    } catch {
      return String(v);
    }
  }
</script>

<header class="hero">
  <h1>Chat</h1>
  <p class="tagline">
    Optional assistant grounded in the bundled lens content. Off by default.
    {#if !llmState.loaded}
      <span class="muted">Checking status…</span>
    {:else if !llmState.status?.configured}
      <a href="/settings/llm">Configure a provider →</a>
    {:else}
      <span class="muted">
        Using <code>{llmState.status.provider}</code> ·
        <code>{llmState.status.model}</code>.
        <a href="/settings/llm">Change</a>
      </span>
    {/if}
  </p>
</header>

<div class="warn">
  Verify rules-touching outputs against canonical PF2e Remaster sources before
  bringing them to the table. The model can produce plausible but wrong
  encounter math, statblocks, or rules interactions.
</div>

<section class="thread">
  {#each messages as m, i (i)}
    <div class="msg" data-role={m.role}>
      <div class="role">{m.role}</div>
      {#if m.role === "assistant" && m.traces && m.traces.length > 0 && showContext}
        <ul class="traces">
          {#each m.traces as t (t.id)}
            <li class="trace" data-status={t.error ? "error" : t.result === undefined ? "pending" : "ok"}>
              <details>
                <summary>
                  <span class="tname">{t.name}</span>
                  <span class="targs">{summarizeInput(t.input)}</span>
                  <span class="tstatus">
                    {#if t.error}error{:else if t.result === undefined}calling…{:else}ok{/if}
                  </span>
                </summary>
                <pre class="tresult">{JSON.stringify(t.result ?? t.input, null, 2)}</pre>
              </details>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="content">{m.content || (streaming && i === messages.length - 1 ? "…" : "")}</div>
    </div>
  {:else}
    <p class="empty">
      No messages yet. Ask something like
      <em>"Build a moderate encounter for a party of 4 at level 6"</em>
      or
      <em>"What does the Lewisian lens say about magic?"</em>
    </p>
  {/each}
</section>

{#if messages.some((m) => m.traces && m.traces.length > 0)}
  <label class="show-context">
    <input type="checkbox" bind:checked={showContext} />
    Show context (tool calls)
  </label>
{/if}

{#if error}
  <p class="err">{error}</p>
{/if}

<form class="composer" onsubmit={send}>
  <textarea
    bind:value={draft}
    placeholder={llmState.status?.configured
      ? "Ask the companion anything"
      : "Configure /settings/llm first"}
    rows="2"
    onkeydown={(e) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        send(e);
      }
    }}
    disabled={!llmState.status?.configured || streaming}
  ></textarea>
  <div class="actions">
    <button
      type="submit"
      class="primary"
      disabled={!llmState.status?.configured || streaming || !draft.trim()}
    >
      {streaming ? "…" : "Send"}
    </button>
    <button type="button" onclick={reset} disabled={streaming || messages.length === 0}>
      Reset
    </button>
    {#if lastUsage}
      <span class="usage">
        in {lastUsage.input ?? "?"} / out {lastUsage.output ?? "?"} tokens
        {#if lastUsage.iterations && lastUsage.iterations > 1}
          · {lastUsage.iterations} turns
        {/if}
        {#if lastUsage.cacheRead}
          · cache hit {lastUsage.cacheRead}
        {/if}
      </span>
    {/if}
  </div>
</form>

<style>
  .hero {
    margin-bottom: 0.6rem;
  }
  h1 {
    margin: 0;
    font-size: 1.4rem;
    font-weight: 600;
  }
  .tagline {
    margin: 0.15rem 0 0;
    color: var(--muted);
    font-size: 0.85rem;
  }
  .tagline a {
    color: inherit;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, currentColor 30%, transparent);
  }
  .muted {
    color: var(--muted);
  }
  code {
    background: var(--bg-soft);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
    font-size: 0.85em;
  }

  .warn {
    margin: 0.6rem 0 1rem;
    padding: 0.55rem 0.85rem;
    border-radius: 8px;
    background: color-mix(in srgb, hsl(45 80% 50%) 8%, transparent);
    border: 1px solid color-mix(in srgb, hsl(45 80% 50%) 35%, var(--line));
    color: color-mix(in srgb, hsl(45 80% 35%) 80%, currentColor);
    font-size: 0.82rem;
    line-height: 1.45;
  }

  .thread {
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
    margin: 0.5rem 0 1rem;
    min-height: 35vh;
  }
  .msg {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .msg .role {
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .msg .content {
    padding: 0.65rem 0.85rem;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    white-space: pre-wrap;
    line-height: 1.5;
    font-size: 0.92rem;
  }
  .msg[data-role="user"] .content {
    background: color-mix(in srgb, currentColor 7%, transparent);
  }
  .empty {
    color: var(--muted);
    text-align: center;
    padding: 1.5rem 1rem;
    border: 1px dashed var(--line);
    border-radius: 12px;
  }
  .traces {
    list-style: none;
    margin: 0.2rem 0 0.45rem;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .trace {
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg-soft);
    font-size: 0.78rem;
  }
  .trace[data-status="pending"] {
    border-color: color-mix(in srgb, hsl(45 85% 50%) 35%, var(--line));
  }
  .trace[data-status="error"] {
    border-color: color-mix(in srgb, hsl(0 65% 50%) 35%, var(--line));
  }
  .trace summary {
    list-style: none;
    cursor: pointer;
    padding: 0.35rem 0.6rem;
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }
  .trace summary::-webkit-details-marker {
    display: none;
  }
  .tname {
    font-weight: 500;
    font-family:
      ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  }
  .targs {
    color: var(--muted);
    font-family:
      ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 0.72rem;
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .tstatus {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .trace[data-status="error"] .tstatus {
    color: #c33;
  }
  .tresult {
    margin: 0;
    padding: 0.55rem 0.7rem;
    border-top: 1px solid var(--line);
    background: color-mix(in srgb, currentColor 3%, transparent);
    font-family:
      ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    font-size: 0.72rem;
    line-height: 1.4;
    white-space: pre-wrap;
    max-height: 14rem;
    overflow: auto;
  }
  .show-context {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.78rem;
    color: var(--muted);
    margin: -0.4rem 0 0.8rem;
  }
  .empty em {
    color: inherit;
  }

  .composer {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    border-top: 1px solid var(--line);
    padding-top: 0.7rem;
  }
  textarea {
    font: inherit;
    padding: 0.5rem 0.7rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
    resize: vertical;
    min-height: 2.6rem;
  }
  .actions {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }
  button {
    font: inherit;
    padding: 0.5rem 0.9rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
    cursor: pointer;
  }
  button.primary {
    background: color-mix(in srgb, currentColor 12%, transparent);
    border-color: color-mix(in srgb, currentColor 30%, transparent);
    font-weight: 500;
  }
  button[disabled] {
    opacity: 0.55;
    cursor: not-allowed;
  }
  .usage {
    margin-left: auto;
    color: var(--muted);
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
  }

  .err {
    color: #c33;
    padding: 0.5rem 0.75rem;
    border: 1px solid #c33;
    border-radius: 6px;
    background: rgba(204, 51, 51, 0.05);
    font-size: 0.85rem;
  }
</style>
