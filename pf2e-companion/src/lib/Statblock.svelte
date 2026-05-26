<script lang="ts">
  let { statblock }: { statblock: Record<string, unknown> } = $props();

  // Helpers — derive on the prop so they react if it changes.
  const get = (k: string) => statblock[k];
  const arr = (k: string) =>
    Array.isArray(statblock[k])
      ? (statblock[k] as unknown[]).map(String)
      : [];

  let cleric_spells = $derived(
    Array.isArray(statblock.cleric_spells)
      ? (statblock.cleric_spells as Array<{ rank: number; name: string }>)
      : [],
  );
</script>

<aside class="statblock">
  <header>
    <span class="kind">stat block</span>
    {#if get("name")}<h3>{String(get("name"))}</h3>{/if}
    {#if get("title")}<p class="subtitle">{String(get("title"))}</p>{/if}
  </header>

  {#if get("level") !== undefined}
    <div class="row">
      <span class="lbl">Level</span>
      <span>{String(get("level"))}</span>
    </div>
  {/if}

  {#if arr("edicts").length}
    <section>
      <h4>Edicts</h4>
      <ul class="lined">
        {#each arr("edicts") as edict (edict)}
          <li>{edict}</li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if arr("anathema").length}
    <section>
      <h4>Anathema</h4>
      <ul class="lined">
        {#each arr("anathema") as anath (anath)}
          <li>{anath}</li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if arr("areas_of_concern").length}
    <div class="row">
      <span class="lbl">Areas of concern</span>
      <span>{arr("areas_of_concern").join(", ")}</span>
    </div>
  {/if}

  {#if get("divine_attribute") !== undefined}
    <div class="row">
      <span class="lbl">Divine attribute</span>
      <span>
        {Array.isArray(get("divine_attribute"))
          ? (get("divine_attribute") as string[]).join(" or ")
          : String(get("divine_attribute"))}
      </span>
    </div>
  {/if}
  {#if get("divine_font")}
    <div class="row">
      <span class="lbl">Divine font</span>
      <span>{String(get("divine_font"))}</span>
    </div>
  {/if}
  {#if get("sanctification")}
    <div class="row">
      <span class="lbl">Sanctification</span>
      <span class="sanct" data-s={String(get("sanctification"))}>
        {String(get("sanctification"))}
      </span>
    </div>
  {/if}
  {#if get("divine_skill")}
    <div class="row">
      <span class="lbl">Divine skill</span>
      <span>{String(get("divine_skill"))}</span>
    </div>
  {/if}
  {#if get("favored_weapon")}
    <div class="row">
      <span class="lbl">Favored weapon</span>
      <span>{String(get("favored_weapon"))}</span>
    </div>
  {/if}
  {#if arr("domains").length}
    <div class="row">
      <span class="lbl">Domains</span>
      <span>{arr("domains").join(", ")}</span>
    </div>
  {/if}
  {#if arr("alternate_domains").length}
    <div class="row">
      <span class="lbl">Alternate</span>
      <span class="muted">{arr("alternate_domains").join(", ")}</span>
    </div>
  {/if}

  {#if cleric_spells.length}
    <section>
      <h4>Cleric spells</h4>
      <ul class="lined">
        {#each cleric_spells as cs (cs.rank + cs.name)}
          <li>
            <span class="rank">{cs.rank}</span>
            <em>{cs.name}</em>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if get("religious_symbol") || get("sacred_animal") || arr("sacred_colors").length}
    <section class="iconography">
      {#if get("religious_symbol")}
        <div class="row"><span class="lbl">Symbol</span><span>{String(get("religious_symbol"))}</span></div>
      {/if}
      {#if get("sacred_animal")}
        <div class="row"><span class="lbl">Animal</span><span>{String(get("sacred_animal"))}</span></div>
      {/if}
      {#if arr("sacred_colors").length}
        <div class="row"><span class="lbl">Colors</span><span>{arr("sacred_colors").join(", ")}</span></div>
      {/if}
    </section>
  {/if}
</aside>

<style>
  .statblock {
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--bg-soft);
    padding: 0.85rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
    font-size: 0.88rem;
  }
  header {
    border-bottom: 1px solid var(--line);
    padding-bottom: 0.45rem;
  }
  .kind {
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
  }
  h3 {
    margin: 0.1rem 0 0;
    font-size: 1.05rem;
  }
  h4 {
    margin: 0.6rem 0 0.25rem;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    font-weight: 600;
  }
  .subtitle {
    margin: 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .row {
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
  }
  .lbl {
    color: var(--muted);
    font-size: 0.75rem;
    width: 7rem;
    flex: 0 0 auto;
  }
  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
  ul.lined {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  ul.lined li {
    font-size: 0.85rem;
  }
  .rank {
    display: inline-block;
    min-width: 1rem;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .sanct[data-s*="holy"]:not([data-s*="unholy"]) {
    color: hsl(45 80% 45%);
  }
  .sanct[data-s*="unholy"] {
    color: hsl(280 50% 50%);
  }
  .sanct[data-s="none"] {
    color: var(--muted);
    font-style: italic;
  }
  section.iconography {
    margin-top: 0.4rem;
    border-top: 1px solid var(--line);
    padding-top: 0.45rem;
  }
</style>
