<script setup lang="ts">
import { BOLD, ITALIC } from "../../lib/highlight";
import type { DiffLine } from "../../lib/unifiedDiff";

/**
 * What changed in a file since the last commit, hunk by hunk: removed lines over the lines that
 * replaced them, each with its number on its side. A line of the file as it is now takes the
 * file's colours (`tokens`, by its number); a removed one, gone from the file, is plain.
 */
const props = defineProps<{ lines: DiffLine[]; palette: string[]; tokens: number[][] | null }>();
const emit = defineEmits<{ line: [line: number] }>();

type Piece = { text: string; style: string };

function pieces(line: DiffLine): Piece[] {
  const tokens = line.new !== null ? props.tokens?.[line.new - 1] : null;
  if (!tokens) return [{ text: line.text, style: "" }];
  const out: Piece[] = [];
  let at = 0;
  for (let i = 0; i < tokens.length && at < line.text.length; i += 3) {
    const font = tokens[i + 2];
    const colour = props.palette[tokens[i + 1]] ?? "";
    out.push({ text: line.text.slice(at, at + tokens[i]), style: `${colour ? `color:${colour};` : ""}${font & ITALIC ? "font-style:italic;" : ""}${font & BOLD ? "font-weight:600;" : ""}` });
    at += tokens[i];
  }
  if (at < line.text.length) out.push({ text: line.text.slice(at), style: "" });
  return out;
}

const SIGN = { add: "+", del: "−", context: " ", hunk: "" } as const;
</script>

<template>
  <div class="diff-view">
    <div class="lines selectable">
      <template v-for="(line, i) in lines" :key="i">
        <div v-if="line.kind === 'hunk'" class="hunk" aria-hidden="true">{{ line.text }}</div>
        <div v-else :class="['ln', line.kind]" @click="line.new !== null && emit('line', line.new)">
          <span class="gut" aria-hidden="true"><span class="num">{{ line.old ?? "" }}</span><span class="num">{{ line.new ?? "" }}</span><span class="sign">{{ SIGN[line.kind] }}</span></span>
          <span class="code"><span v-for="(piece, k) in pieces(line)" :key="k" :style="piece.style">{{ piece.text }}</span></span>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.diff-view {
  flex: 1;
  min-height: 0;
  overflow: auto;
  background: var(--bg-log);
  font-family: var(--font-mono);
  font-size: 12.5px;
  line-height: 19px;
}

.lines {
  width: max-content;
  min-width: 100%;
  padding: 4px 0 40px;
}

.hunk {
  padding: 6px 16px 4px;
  font-size: 11.5px;
  color: var(--text-faint);
  -webkit-user-select: none;
  user-select: none;
}

.ln {
  display: flex;
  height: 19px;
}

.ln.add,
.ln.add .gut {
  background: var(--bg-diff-add);
}

.ln.del,
.ln.del .gut {
  background: var(--bg-diff-del);
}

.gut {
  position: sticky;
  left: 0;
  display: flex;
  flex-shrink: 0;
  background: var(--bg-log);
  color: var(--text-faint);
  -webkit-user-select: none;
  user-select: none;
}

.num {
  width: 44px;
  padding-right: 8px;
  box-sizing: border-box;
  text-align: right;
  opacity: 0.75;
}

.sign {
  width: 22px;
  text-align: center;
}

.add .sign {
  color: var(--run);
}

.del .sign {
  color: var(--crash);
}

.code {
  white-space: pre;
  padding-right: 24px;
  color: var(--text);
}

.del .code {
  color: var(--text-muted);
}
</style>
