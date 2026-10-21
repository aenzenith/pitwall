<script setup lang="ts">
import ClaudeDot from "../../components/ClaudeDot.vue";
import type { Lane, Live, Tick } from "../../lib/day";
import { t } from "../../lib/i18n";
import { openInWindow, openTarget } from "../../lib/open";
import { useTooltip } from "../../lib/tooltip";

/** One project's row: its name and time, and what happened on it along the hours. `on`: it is the
 * selected project; `nowLeft`: where now is, while today is shown; `live`: Claude, right now. */
const props = defineProps<{ lane: Lane; on: boolean; ticks: Tick[]; nowLeft: string | null; live?: Live }>();
const emit = defineEmits<{ select: [] }>();

/** The hint on its name: what a double click opens (lib/open). */
const LANE_TITLES = { list: "day.laneTitleList", editor: "day.laneTitle", files: "day.laneTitleFiles" } as const;

const tooltip = useTooltip();
/** The block the arrows are on, oldest first; `null` once the tooltip goes. */
let stepping: number | null = null;
tooltip.onHide(() => (stepping = null));

/** ←/→ (Home, End) step through what happened on the lane, oldest first, each with its tooltip; Esc puts it away. */
function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    if (tooltip.tip.value) {
      event.preventDefault();
      tooltip.hide();
    }
    return;
  }
  if (!["ArrowRight", "ArrowLeft", "Home", "End"].includes(event.key)) return;
  const steps = [...props.lane.blocks].sort((a, b) => a.at - b.at);
  if (!steps.length) return;
  event.preventDefault();
  const last = steps.length - 1;
  const from = stepping ?? -1;
  const at =
    event.key === "Home" ? 0 : event.key === "End" ? last : event.key === "ArrowRight" ? Math.min(last, from + 1) : from < 0 ? last : Math.max(0, from - 1);
  const block = steps[at];
  const element = (event.currentTarget as HTMLElement).querySelector(`[data-block="${block.key}"]`);
  if (!element) return;
  tooltip.showAt(element, block.tip);
  tooltip.announce(block.tip);
  stepping = at;
}
</script>

<template>
  <button
    type="button"
    :class="['lane', { on }]"
    :aria-pressed="on"
    :aria-describedby="lane.blocks.length ? 'lane-keys' : undefined"
    @click="emit('select')"
    @dblclick="openInWindow(lane.project.path, 'pages')"
    @keydown="onKey"
    @blur="tooltip.hide()"
  >
    <!-- The hint sits on the name only: the blocks have their own tooltips. -->
    <span class="label" :title="t(LANE_TITLES[openTarget(lane.project.path, 'pages')])">
      <span class="mark"><ClaudeDot :live="live" /></span>
      <span class="names">
        <span class="name">{{ lane.project.name }}</span>
        <span v-if="lane.total" class="lane-total">{{ lane.total }}</span>
        <span v-if="lane.tokens" class="lane-total">{{ lane.tokens }}<template v-if="lane.week"> · <span class="lane-week">{{ lane.week }}</span></template></span>
      </span>
    </span>
    <!-- Drawn for the eye; VoiceOver hears each block as the arrows reach it. -->
    <span class="track" aria-hidden="true">
      <span v-for="tick in ticks" :key="tick.at" :class="['grid', { major: tick.major }]" :style="{ left: tick.left }"></span>
      <span
        v-for="block in lane.blocks"
        :key="block.key"
        :data-block="block.key"
        :class="['block', block.kind]"
        :style="block.style"
        @pointerenter="tooltip.show($event, block.tip)"
        @pointerleave="tooltip.hide()"
      >
        <span v-if="block.text" class="text">{{ block.text }}</span>
      </span>
      <span v-if="nowLeft" class="now" :style="{ left: nowLeft }"></span>
    </span>
  </button>
</template>

<style scoped>
/* The whole row picks the project; no colours, the row just lights up. */
.lane {
  box-sizing: border-box;
  display: flex;
  align-items: center;
  gap: 18px;
  width: 100%;
  flex-shrink: 0;
  margin: 0;
  padding: 6px 10px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
}

.lane:hover {
  background: var(--bg-lane-hover);
}

.lane.on {
  background: var(--bg-lane-on);
}

/* The ring inside: the list clips anything past its sides. */
.lane:focus-visible {
  outline-offset: -2px;
}

/* Claude's dot before the name and its time, centred on the two, spaced as the project list
   spaces its status: in a 16px slot, 10 from the row's edge and 10 from the name. */
.label {
  width: 160px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.mark {
  width: 16px;
  flex-shrink: 0;
  display: inline-flex;
  justify-content: center;
}

.names {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.name {
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.lane-total {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-subtle);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.lane-week {
  color: var(--claude-text);
}

/* Square, like the blocks on it. */
.track {
  position: relative;
  flex-grow: 1;
  height: 46px;
  background: var(--bg-timeline);
  box-shadow: inset 0 0 0 1px var(--line-soft);
}

.lane.on .track {
  background: var(--bg-timeline-on);
  box-shadow: inset 0 0 0 1px var(--line-selected);
}

.grid {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 1px;
  background: var(--grid);
  pointer-events: none;
}

.grid.major {
  background: var(--grid-major);
}

.now {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 2px;
  margin-left: -1px;
  background: var(--run);
  opacity: 0.85;
  pointer-events: none;
}

.block {
  position: absolute;
  box-sizing: border-box;
}

/* A thin rail, with room around it to point at. */
.block.server {
  bottom: 3px;
  height: 12px;
}

.block.server::before {
  content: "";
  position: absolute;
  left: 0;
  right: 0;
  top: 4px;
  height: 4px;
  background: var(--run);
  box-shadow: 0 0 6px var(--run-glow);
}

.block.work,
.block.wait {
  top: 18px;
  height: 16px;
  overflow: hidden;
  container-type: inline-size;
}

.block.work {
  background: var(--work);
  box-shadow: var(--work-shine);
}

.block.wait {
  border: 1px dashed color-mix(in srgb, var(--claude) 85%, transparent);
  background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--claude) 32%, transparent) 0 3px, transparent 3px 7px);
}

.text {
  display: block;
  padding: 0 6px;
  font-size: 10px;
  line-height: 16px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.block.work .text {
  font-weight: 600;
  color: var(--work-text);
}

.block.wait .text {
  padding: 0 2px;
  font-family: var(--font-mono);
  line-height: 14px;
  text-align: center;
  color: var(--claude-soft);
}

/* Too narrow to read: no text at all rather than an ellipsis. */
@container (max-width: 60px) {
  .block.work .text {
    display: none;
  }
}

@container (max-width: 34px) {
  .block.wait .text {
    display: none;
  }
}

.block.commit {
  top: 2px;
  width: 14px;
  height: 14px;
  margin-left: -7px;
}

.block.commit::before {
  content: "";
  position: absolute;
  left: 3px;
  top: 3px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-strong);
  box-shadow:
    0 0 0 2px var(--bg-timeline),
    0 0 0 3px var(--line-heavy);
}

.lane.on .block.commit::before {
  box-shadow:
    0 0 0 2px var(--bg-timeline-on),
    0 0 0 3px var(--line-heavy);
}

.block.crash {
  bottom: 1px;
  width: 14px;
  height: 14px;
  margin-left: -7px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--crash-bg);
  box-shadow: 0 0 0 1px var(--crash-line);
  color: var(--crash-text);
  font-size: 9px;
  font-weight: 700;
}

.block.crash .text {
  padding: 0;
  line-height: 14px;
}
</style>
