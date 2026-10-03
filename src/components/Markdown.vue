<script lang="ts">
import { computed, defineComponent, h, type VNode } from "vue";

import { blocks, type Block, type Span } from "../lib/markdown";

const TAGS = { strong: "strong", em: "em", strike: "s" } as const;

function inline(spans: Span[]): (VNode | string)[] {
  return spans.map((span) => ("spans" in span ? h(TAGS[span.kind], inline(span.spans)) : span.kind === "code" ? h("code", span.text) : span.text));
}

function row(cells: Span[][], tag: "th" | "td"): VNode {
  return h(
    "tr",
    cells.map((cell) => h(tag, inline(cell))),
  );
}

function block(b: Block): VNode {
  switch (b.kind) {
    case "heading":
      return h("div", { class: ["md-heading", { top: b.level <= 2 }], role: "heading", "aria-level": b.level }, inline(b.spans));
    case "paragraph":
      return h("p", { class: "md-text" }, inline(b.spans));
    case "item":
      return h("div", { class: "md-item", style: { paddingLeft: `${b.depth * 16}px` } }, [h("span", { class: "md-marker" }, b.marker), h("span", { class: "md-body" }, inline(b.spans))]);
    case "quote":
      return h("p", { class: "md-text md-quote" }, inline(b.spans));
    case "code":
      return h("pre", { class: "md-code" }, b.text);
    case "table":
      return h("div", { class: "md-table" }, h("table", [b.head ? h("thead", row(b.head, "th")) : null, h("tbody", b.rows.map((cells) => row(cells, "td")))]));
    case "rule":
      return h("hr", { class: "md-rule" });
  }
}

/**
 * Markdown as Claude writes it (lib/markdown), drawn as elements: nothing in the text is taken as
 * HTML, a link shows only its text and nothing is fetched. Output, so it can be selected.
 */
export default defineComponent({
  props: { text: { type: String, required: true } },
  setup(props) {
    const parsed = computed(() => blocks(props.text));
    return () => h("div", { class: "md selectable" }, parsed.value.map(block));
  },
});
</script>

<style scoped>
.md {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

.md-text {
  margin: 0;
  white-space: pre-line;
}

.md-heading {
  font-weight: 600;
  color: var(--text);
}

.md-heading.top {
  font-size: 14px;
}

.md-heading:not(:first-child) {
  margin-top: 4px;
}

.md-item {
  display: flex;
  gap: 6px;
  white-space: pre-line;
}

/* A list's items sit closer than its paragraphs. */
.md-item + .md-item {
  margin-top: -4px;
}

.md-marker {
  flex-shrink: 0;
  min-width: 14px;
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
}

.md-body {
  min-width: 0;
}

.md strong {
  font-weight: 600;
  color: var(--text);
}

.md code {
  padding: 1px 4px;
  border-radius: 4px;
  background: var(--bg-control);
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text);
}

.md-quote {
  font-style: italic;
  color: var(--text-subtle);
}

/* The terminal's screen (sessions/SessionTerminal); a long line scrolls in it. */
.md-code {
  margin: 0;
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--bg-log);
  border: 1px solid #1f2228;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.5;
  color: var(--text);
  white-space: pre;
  overflow-x: auto;
  overflow-wrap: normal;
}

.md-rule {
  width: 100%;
  margin: 2px 0;
  border: 0;
  border-top: 1px solid var(--line);
}

/* A wide table scrolls, its words kept whole. */
.md-table {
  overflow-x: auto;
  overflow-wrap: normal;
}

.md-table table {
  border-collapse: collapse;
  font-size: 12px;
}

.md-table th,
.md-table td {
  padding: 4px 12px 4px 0;
  border-bottom: 1px solid var(--line);
  text-align: left;
  vertical-align: top;
}

.md-table th {
  font-weight: 600;
  color: var(--text);
}
</style>
