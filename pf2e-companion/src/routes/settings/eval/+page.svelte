<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import {
    evalLoadSuite,
    evalRun,
    type EvalPrompt,
    type SuiteSummary,
  } from "$lib/ipc";
  import { llmState, refreshLlmStatus } from "$lib/llm.svelte";

  let suite = $state<EvalPrompt[]>([]);
  let loading = $state(true);
  let running = $state(false);
  let summary = $state<SuiteSummary | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    await Promise.all([refreshLlmStatus(), loadSuite()]);
  });

  async function loadSuite() {
    loading = true;
    try {
      suite = await evalLoadSuite();
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function runSuite() {
    if (running || !llmState.status?.configured) return;
    running = true;
    summary = null;
    error = null;
    try {
      summary = await evalRun();
    } catch (e) {
      error = String(e);
    } finally {
      running = false;
    }
  }

  function expectationLabel(e: EvalPrompt["expectations"][number]): string {
    switch (e.kind) {
      case "tool_called":
        return `calls ${e.name}`;
      case "tool_called_with":
        return `calls ${e.name} with ${JSON.stringify(e.input_contains)}`;
      case "text_contains":
        return `says "${e.needle}"`;
      case "text_excludes":
        return `does not say "${e.needle}"`;
      case "iterations_at_most":
        return `≤ ${e.max} iterations`;
    }
  }
</script>

<header class="hero">
  <button
    type="button"
    class="back"
    onclick={() => (history.length > 1 ? history.back() : goto("/"))}
  >
    ← Back
  </button>
  <h1>Eval harness</h1>
  <p class="tagline">
    Smoke tests for the LLM agent. Each prompt runs through the agent loop;
    the harness grades the output (tool calls, text, iteration count).
    Useful when you tune the system prompt or add a new tool — regressions
    show up here immediately.
  </p>
</header>

<section class="actions">
  <button
    type="button"
    class="primary"
    onclick={runSuite}
    disabled={running || !llmState.status?.configured}
  >
    {running ? "Running suite…" : "Run suite"}
  </button>
  {#if !llmState.status?.configured}
    <span class="muted">
      Configure a provider at <a href="/settings/llm">/settings/llm</a> first.
    </span>
  {:else}
    <span class="muted">
      Using <code>{llmState.status.provider}</code> ·
      <code>{llmState.status.model}</code>. The agent has full tool access.
    </span>
  {/if}
</section>

{#if error}
  <p class="err">{error}</p>
{/if}

{#if summary}
  <section class="summary" data-status={summary.failed === 0 ? "ok" : "fail"}>
    <strong>{summary.passed} / {summary.total}</strong> passed
    {#if summary.failed > 0}
      · <span class="failed">{summary.failed} failed</span>
    {/if}
  </section>
{/if}

<section class="suite">
  {#if loading}
    <p class="muted">Loading suite…</p>
  {:else if suite.length === 0}
    <p class="muted">Suite is empty.</p>
  {:else}
    <ul class="prompts">
      {#each suite as p (p.id)}
        {@const result = summary?.results.find((r) => r.id === p.id) ?? null}
        <li class="prompt" data-status={result ? (result.passed ? "ok" : "fail") : "pending"}>
          <header class="phead">
            <code class="pid">{p.id}</code>
            {#if result}
              <span class="status">
                {result.passed ? "PASS" : "FAIL"}
              </span>
            {/if}
          </header>
          <p class="pdesc">{p.description}</p>
          <p class="pquery">"{p.prompt}"</p>
          <ul class="expectations">
            {#each p.expectations as e, i (i)}
              <li>{expectationLabel(e)}</li>
            {/each}
          </ul>
          {#if result}
            {#if result.failures.length > 0}
              <ul class="failures">
                {#each result.failures as f, i (i)}
                  <li>{f}</li>
                {/each}
              </ul>
            {/if}
            <details>
              <summary>
                Run details · {result.iterations} iter ·
                {result.tool_calls.length} tool calls
              </summary>
              <div class="run-details">
                {#if result.tool_calls.length > 0}
                  <h4>Tool calls</h4>
                  <ol>
                    {#each result.tool_calls as t, i (i)}
                      <li>
                        <code>{t.name}</code>
                        <code class="args">{JSON.stringify(t.input)}</code>
                      </li>
                    {/each}
                  </ol>
                {/if}
                {#if result.final_text}
                  <h4>Final text</h4>
                  <pre>{result.final_text}</pre>
                {/if}
                {#if result.error}
                  <h4>Error</h4>
                  <pre class="err">{result.error}</pre>
                {/if}
              </div>
            </details>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .hero {
    margin-bottom: 1rem;
  }
  .back {
    align-self: flex-start;
    font: inherit;
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0.25rem 0;
    margin-bottom: 0.4rem;
    font-size: 0.85rem;
  }
  h1 {
    margin: 0.1rem 0 0;
    font-size: 1.4rem;
    font-weight: 600;
  }
  .tagline {
    margin: 0.2rem 0 0;
    color: var(--muted);
    font-size: 0.85rem;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin: 0.8rem 0;
    flex-wrap: wrap;
  }
  .actions button {
    font: inherit;
    padding: 0.55rem 1rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
    cursor: pointer;
  }
  .actions .primary {
    background: color-mix(in srgb, currentColor 12%, transparent);
    border-color: color-mix(in srgb, currentColor 30%, transparent);
    font-weight: 500;
  }
  .actions button[disabled] {
    opacity: 0.55;
    cursor: not-allowed;
  }
  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .muted a {
    color: inherit;
    text-decoration: underline;
  }
  code {
    background: var(--bg-soft);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
    font-size: 0.85em;
    font-family:
      ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  }

  .summary {
    margin: 0.8rem 0 1rem;
    padding: 0.6rem 0.85rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    font-size: 0.9rem;
  }
  .summary[data-status="ok"] {
    background: color-mix(in srgb, hsl(140 60% 45%) 12%, transparent);
    border-color: color-mix(in srgb, hsl(140 60% 45%) 35%, var(--line));
  }
  .summary[data-status="fail"] {
    background: color-mix(in srgb, hsl(0 65% 50%) 10%, transparent);
    border-color: color-mix(in srgb, hsl(0 65% 50%) 35%, var(--line));
  }
  .summary .failed {
    color: #c33;
  }

  .prompts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  .prompt {
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--bg-soft);
    padding: 0.7rem 0.85rem;
  }
  .prompt[data-status="ok"] {
    border-color: color-mix(in srgb, hsl(140 60% 45%) 35%, var(--line));
  }
  .prompt[data-status="fail"] {
    border-color: color-mix(in srgb, hsl(0 65% 50%) 40%, var(--line));
  }
  .phead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .pid {
    font-size: 0.78rem;
  }
  .status {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 0.15rem 0.5rem;
    border-radius: 999px;
    border: 1px solid var(--line);
  }
  .prompt[data-status="ok"] .status {
    color: hsl(140 60% 35%);
    border-color: color-mix(in srgb, hsl(140 60% 45%) 40%, var(--line));
  }
  .prompt[data-status="fail"] .status {
    color: #c33;
    border-color: color-mix(in srgb, hsl(0 65% 50%) 40%, var(--line));
  }
  .pdesc {
    margin: 0.35rem 0 0.2rem;
    font-size: 0.85rem;
  }
  .pquery {
    margin: 0 0 0.4rem;
    color: var(--muted);
    font-size: 0.82rem;
    font-style: italic;
  }
  .expectations {
    list-style: square;
    margin: 0.2rem 0 0.4rem 1rem;
    padding: 0;
    font-size: 0.78rem;
    color: var(--muted);
  }
  .failures {
    list-style: none;
    margin: 0.4rem 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .failures li {
    color: #c33;
    font-size: 0.82rem;
    padding: 0.3rem 0.5rem;
    background: rgba(204, 51, 51, 0.06);
    border-radius: 6px;
  }
  details summary {
    cursor: pointer;
    font-size: 0.78rem;
    color: var(--muted);
    padding: 0.3rem 0;
  }
  .run-details {
    margin-top: 0.4rem;
    padding-top: 0.4rem;
    border-top: 1px solid var(--line);
    font-size: 0.82rem;
  }
  .run-details h4 {
    margin: 0.6rem 0 0.25rem;
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .run-details ol {
    margin: 0;
    padding-left: 1.2rem;
  }
  .run-details ol li {
    margin: 0.15rem 0;
    font-size: 0.78rem;
  }
  .run-details .args {
    color: var(--muted);
    margin-left: 0.4rem;
  }
  .run-details pre {
    margin: 0.2rem 0;
    padding: 0.5rem 0.7rem;
    border-radius: 6px;
    background: color-mix(in srgb, currentColor 4%, transparent);
    border: 1px solid var(--line);
    white-space: pre-wrap;
    font-size: 0.78rem;
    line-height: 1.45;
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
