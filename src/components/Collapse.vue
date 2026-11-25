<script setup lang="ts">
/**
 * What it holds opens and shuts by easing its height, never jumping (the user's word, 16 Nov: the
 * chat's rows "pat diye" changed height). Opening, it eases toward what it holds as that grows
 * (a diff drawn a moment after it came), so it never jumps at the end. Its box has no padding of
 * its own, so it eases down to nothing; put the padding on what it holds. At once under reduced
 * motion. `keep`: shut, what it holds is hidden, not dropped (what was typed in it stays).
 * `fit`: it is a flex column that may be given less height than what it holds, which scrolls
 * inside; what it holds then follows the ease, so the ease doesn't follow what it holds.
 */
const props = defineProps<{ show: boolean; keep?: boolean; fit?: boolean }>();

const EASE = "height 0.2s cubic-bezier(0.2, 0, 0, 1)";

type Run = { done: () => void; timer: ReturnType<typeof setTimeout>; end: (event: TransitionEvent) => void; watcher: ResizeObserver | null };
const runs = new WeakMap<HTMLElement, Run>();

function finish(box: HTMLElement): void {
  const run = runs.get(box);
  if (!run) return;
  runs.delete(box);
  clearTimeout(run.timer);
  box.removeEventListener("transitionend", run.end);
  run.watcher?.disconnect();
  run.done();
}

function ease(box: HTMLElement, from: number, to: number, done: () => void, inner: HTMLElement | null): void {
  if (from === to || matchMedia("(prefers-reduced-motion: reduce)").matches) {
    done();
    return;
  }
  box.style.overflow = "hidden";
  box.style.height = `${from}px`;
  void box.offsetHeight;
  box.style.transition = EASE;
  box.style.height = `${to}px`;
  const end = (event: TransitionEvent): void => {
    if (event.target === box && event.propertyName === "height") finish(box);
  };
  // What it holds grew or shrank on the way: the height eases on toward it.
  const watcher = inner
    ? new ResizeObserver(() => {
        const now = inner.offsetHeight;
        if (`${now}px` !== box.style.height) box.style.height = `${now}px`;
      })
    : null;
  watcher?.observe(inner!);
  // Done all the same should no end come (a box hidden on the way); a new target restarts the
  // ease, so the fallback allows for one.
  runs.set(box, { done, end, watcher, timer: setTimeout(() => finish(box), 450) });
  box.addEventListener("transitionend", end);
}

function enter(el: Element, done: () => void): void {
  const box = el as HTMLElement;
  const inner = box.firstElementChild as HTMLElement | null;
  ease(box, 0, inner?.offsetHeight ?? box.scrollHeight, done, props.fit ? null : inner);
}

function leave(el: Element, done: () => void): void {
  const box = el as HTMLElement;
  ease(box, box.offsetHeight, 0, done, null);
}

function settle(el: Element): void {
  const box = el as HTMLElement;
  finish(box);
  box.style.height = "";
  box.style.overflow = "";
  box.style.transition = "";
}
</script>

<template>
  <Transition :css="false" @enter="enter" @after-enter="settle" @enter-cancelled="settle" @leave="leave" @after-leave="settle" @leave-cancelled="settle">
    <div v-if="keep || show" v-show="!keep || show" :class="['collapse', { fit }]"><div class="held"><slot /></div></div>
  </Transition>
</template>

<style scoped>
/* Its children's margins stay inside what it holds, eased or not. */
.held {
  display: flow-root;
}

.fit,
.fit > .held {
  display: flex;
  flex-direction: column;
  min-height: 0;
}
</style>
