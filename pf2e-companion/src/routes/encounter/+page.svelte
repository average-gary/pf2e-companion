<script lang="ts">
  import {
    xpBudget,
    creatureXp,
    type Difficulty,
    type XpBudgetResult,
  } from "$lib/ipc";

  const difficulties: { id: Difficulty; label: string }[] = [
    { id: "trivial", label: "Trivial" },
    { id: "low", label: "Low" },
    { id: "moderate", label: "Moderate" },
    { id: "severe", label: "Severe" },
    { id: "extreme", label: "Extreme" },
  ];

  let partySize = $state(4);
  let partyLevel = $state(5);
  let difficulty = $state<Difficulty>("moderate");
  let result = $state<XpBudgetResult | null>(null);
  let creatureTable = $state<{ delta: number; xp: number | null }[]>([]);
  let error = $state<string | null>(null);

  async function recompute() {
    error = null;
    try {
      result = await xpBudget(partySize, difficulty);
      const rows = await Promise.all(
        [-4, -3, -2, -1, 0, 1, 2, 3, 4].map(async (d) => ({
          delta: d,
          xp: await creatureXp(d),
        })),
      );
      creatureTable = rows;
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    void [partySize, partyLevel, difficulty];
    recompute();
  });
</script>

<section class="hero">
  <h1>Encounter Budget</h1>
  <p class="tagline">PF2e Remaster · GM Core p.49</p>
</section>

<div class="grid">
  <label>
    <span class="lbl">Party size</span>
    <input
      type="number"
      min="1"
      max="12"
      bind:value={partySize}
    />
  </label>

  <label>
    <span class="lbl">Party level</span>
    <input
      type="number"
      min="1"
      max="20"
      bind:value={partyLevel}
    />
  </label>

  <label class="span">
    <span class="lbl">Difficulty</span>
    <div class="pills">
      {#each difficulties as d (d.id)}
        <button
          type="button"
          class="pill"
          class:active={difficulty === d.id}
          onclick={() => (difficulty = d.id)}
        >
          {d.label}
        </button>
      {/each}
    </div>
  </label>
</div>

{#if error}
  <p class="err">{error}</p>
{:else if result}
  <section class="result">
    <div class="big">
      <span class="num">{result.xp_budget}</span>
      <span class="unit">XP</span>
    </div>
    <p class="caption">
      Base for a party of 4: <strong>{result.base_for_party_of_4}</strong>
      &nbsp;·&nbsp;
      Per-PC adjust: <strong>±{result.per_pc_adjust}</strong>
    </p>
  </section>

  <section class="table">
    <h2>Creature XP cost (level vs party level)</h2>
    <table>
      <thead>
        <tr>
          <th>Δ</th>
          <th>Creature level</th>
          <th>XP cost</th>
        </tr>
      </thead>
      <tbody>
        {#each creatureTable as row (row.delta)}
          <tr>
            <td class="delta">{row.delta > 0 ? `+${row.delta}` : row.delta}</td>
            <td>{partyLevel + row.delta}</td>
            <td>{row.xp ?? "—"}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{/if}

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

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.75rem;
    margin: 1rem 0;
  }
  .grid .span {
    grid-column: 1 / -1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    font-size: 0.85rem;
  }
  .lbl {
    color: var(--muted);
    font-size: 0.78rem;
  }
  input[type="number"] {
    font: inherit;
    padding: 0.55rem 0.75rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: inherit;
  }

  .pills {
    display: flex;
    gap: 0.35rem;
    flex-wrap: wrap;
  }
  .pill {
    font: inherit;
    font-size: 0.85rem;
    padding: 0.4rem 0.85rem;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    color: var(--muted);
    cursor: pointer;
  }
  .pill.active {
    color: inherit;
    background: color-mix(in srgb, currentColor 12%, transparent);
    border-color: color-mix(in srgb, currentColor 30%, transparent);
  }

  .result {
    margin-top: 1rem;
    padding: 1.25rem;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    text-align: center;
  }
  .big {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 0.4rem;
  }
  .num {
    font-size: 3rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .unit {
    font-size: 1rem;
    color: var(--muted);
  }
  .caption {
    margin: 0.5rem 0 0;
    font-size: 0.85rem;
    color: var(--muted);
  }

  .table {
    margin-top: 1.5rem;
  }
  .table h2 {
    font-size: 0.9rem;
    margin: 0 0 0.5rem;
    color: var(--muted);
    font-weight: 500;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
  }
  th,
  td {
    padding: 0.45rem 0.6rem;
    border-bottom: 1px solid var(--line);
    text-align: left;
  }
  th {
    font-weight: 500;
    color: var(--muted);
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.02em;
  }
  td.delta {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .err {
    color: #c33;
  }
</style>
