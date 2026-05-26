<script lang="ts">
  import { parseSources } from "$lib/citation";

  let { sources }: { sources: string[] } = $props();

  let parsed = $derived(parseSources(sources ?? []));

  function iconFor(kind: string): string {
    switch (kind) {
      case "bible":
        return "📖";
      case "wiki":
        return "🗂";
      case "url":
        return "🔗";
      case "doctrine":
        return "⛪";
      case "patristic":
        return "✒️";
      default:
        return "•";
    }
  }
</script>

{#if parsed.length}
  <section class="sources">
    <header>
      <h4>Sources</h4>
      <a class="legend" href="/about#citations" title="What do these mean?">
        ?
      </a>
    </header>
    <ul>
      {#each parsed as c, i (c.raw + i)}
        <li>
          <span class="icon" aria-hidden="true">{iconFor(c.kind)}</span>
          {#if c.kind === "bible"}
            <a
              href={c.gatewayUrl}
              target="_blank"
              rel="noopener noreferrer"
              title="Open on BibleGateway"
            >
              {c.reference}
            </a>
            {#if c.raw !== c.reference && c.raw.length > c.reference.length}
              <span class="gloss">{c.raw.slice(c.reference.length).trim()}</span>
            {/if}
          {:else if c.kind === "url"}
            <a
              href={c.href}
              target="_blank"
              rel="noopener noreferrer"
              title={c.href}
            >
              {c.label || c.href}
            </a>
          {:else if c.kind === "wiki"}
            <span
              class="wiki"
              title={`Design wiki — ${c.topic}/${c.article}${c.section ? " § " + c.section : ""} (local; the wiki is not bundled with the app)`}
            >
              <span class="topic">{c.topic}</span>
              <span class="sep">/</span>
              <span class="article">{c.article}</span>
              {#if c.section}
                <span class="section">§ {c.section}</span>
              {/if}
            </span>
          {:else if c.kind === "doctrine"}
            <span
              class="doctrine"
              title={`Magisterial / confessional source: ${c.tradition}`}
            >
              {c.label}
            </span>
          {:else if c.kind === "patristic"}
            <span
              class="patristic"
              title={`Theologian / patristic source: ${c.author}`}
            >
              <span class="author">{c.author}</span>
              {#if c.work}
                <span class="work">{c.work}</span>
              {/if}
            </span>
          {:else}
            <span class="plain">{c.text}</span>
          {/if}
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .sources {
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 0.7rem 0.9rem;
    background: var(--bg-soft);
  }
  .sources header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 0.45rem;
  }
  .sources h4 {
    margin: 0;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    font-weight: 600;
  }
  .legend {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 1.1rem;
    height: 1.1rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.7rem;
    text-decoration: none;
    line-height: 1;
  }
  .legend:hover {
    color: inherit;
    border-color: color-mix(in srgb, currentColor 30%, transparent);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.78rem;
    line-height: 1.4;
  }
  li {
    display: grid;
    grid-template-columns: 1.2rem 1fr;
    gap: 0.35rem;
    align-items: baseline;
  }
  .icon {
    font-size: 0.75rem;
    color: var(--muted);
    text-align: center;
  }

  a {
    color: inherit;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, currentColor 30%, transparent);
    text-underline-offset: 0.15em;
  }
  a:hover {
    text-decoration-color: currentColor;
  }

  .wiki {
    color: var(--muted);
    font-family: "SF Mono", Menlo, monospace;
    font-size: 0.72rem;
    cursor: help;
  }
  .wiki .topic {
    color: color-mix(in srgb, currentColor 80%, transparent);
  }
  .wiki .sep {
    color: var(--muted);
    margin: 0 0.1em;
  }
  .wiki .article {
    color: inherit;
  }
  .wiki .section {
    color: var(--muted);
    margin-left: 0.3em;
  }

  .doctrine {
    color: inherit;
    cursor: help;
  }
  .patristic {
    color: inherit;
    cursor: help;
  }
  .patristic .author {
    color: var(--muted);
    margin-right: 0.25em;
  }
  .plain {
    color: var(--muted);
  }
  .gloss {
    color: var(--muted);
    margin-left: 0.25em;
  }
</style>
