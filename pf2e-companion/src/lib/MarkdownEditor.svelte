<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { EditorView, basicSetup } from "codemirror";
  import { EditorState } from "@codemirror/state";
  import { markdown } from "@codemirror/lang-markdown";

  let {
    value = $bindable<string>(""),
    placeholder = "",
    minHeight = "20rem",
  }: {
    value: string;
    placeholder?: string;
    minHeight?: string;
  } = $props();

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let suppressNext = false;

  onMount(() => {
    const state = EditorState.create({
      doc: value,
      extensions: [
        basicSetup,
        markdown(),
        EditorView.lineWrapping,
        EditorView.updateListener.of((u) => {
          if (u.docChanged) {
            const next = u.state.doc.toString();
            suppressNext = true;
            value = next;
          }
        }),
        EditorView.theme({
          "&": { fontSize: "0.92rem", height: "100%" },
          ".cm-content": {
            fontFamily:
              "'SF Mono', Menlo, Monaco, Consolas, 'Liberation Mono', monospace",
            padding: "0.7rem 0.85rem",
            minHeight: minHeight,
          },
          ".cm-scroller": { overflow: "auto" },
          ".cm-focused": { outline: "none" },
        }),
      ],
    });
    view = new EditorView({ state, parent: host });
  });

  onDestroy(() => {
    view?.destroy();
  });

  $effect(() => {
    if (!view) return;
    if (suppressNext) {
      suppressNext = false;
      return;
    }
    const current = view.state.doc.toString();
    if (current !== value) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value },
      });
    }
  });
</script>

<div class="cm-host" bind:this={host} aria-label={placeholder}></div>

<style>
  .cm-host {
    width: 100%;
    min-height: 20rem;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--bg-soft);
    overflow: hidden;
  }
  .cm-host :global(.cm-editor) {
    background: transparent;
    color: inherit;
  }
  .cm-host :global(.cm-gutters) {
    background: transparent;
    border-right: 1px solid var(--line);
    color: var(--muted);
  }
  .cm-host :global(.cm-activeLineGutter),
  .cm-host :global(.cm-activeLine) {
    background: color-mix(in srgb, currentColor 4%, transparent);
  }
  .cm-host :global(.cm-cursor) {
    border-left-color: currentColor;
  }
  .cm-host :global(.cm-selectionBackground),
  .cm-host :global(.cm-content ::selection) {
    background: color-mix(in srgb, currentColor 20%, transparent) !important;
  }
</style>
