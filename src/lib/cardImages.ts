// The images of a card's note: `[Image #n]` in its text, as Claude Code writes one in its prompt,
// and the picture itself, pasted from the clipboard. The core keeps the files; the window holds a
// picture only as a `data:` URL, to draw it and to hand a new one over.

import { computed, ref, watch, type ComputedRef } from "vue";

import { api } from "./store";
import type { Card } from "./types";

/** A card holds this many images at most, each this large at most: the core's limits. */
export const IMAGES_MAX = 12;
export const IMAGE_MB_MAX = 10;

/** The kinds the core takes as they are; another the window can draw is drawn again as a PNG. */
const KEPT_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/** An image as its note's strip draws it; `src` null while it is read. */
export type Shot = { n: number; src: string | null };

/** What stands for image `n` in a note. */
export function imageToken(n: number): string {
  return `[Image #${n}]`;
}

/** The images `note` names, in the order it names them, each once. */
export function imageNumbers(note: string): number[] {
  const found: number[] = [];
  for (const match of note.matchAll(/\[Image #(\d+)\]/g)) {
    const n = Number(match[1]);
    if (n > 0 && !found.includes(n)) found.push(n);
  }
  return found;
}

/** The number a newly pasted image takes: one past every number the note names and every one
 * taken already (`held`), so a number never comes to mean another picture. */
export function nextImageNumber(note: string, held: number[]): number {
  return Math.max(0, ...imageNumbers(note), ...held) + 1;
}

/** `note` with image `n` taken out: each of its tokens, and one of the spaces it stood between. */
export function withoutImage(note: string, n: number): string {
  return note.replace(new RegExp(`( ?)\\[Image #${n}\\]( ?)`, "g"), (_, before: string, after: string) => (before && after ? " " : before + after));
}

/** `token` as it goes into `text` at `at`: a space first when it would touch what is before it. */
export function spaced(text: string, at: number, token: string): string {
  return at > 0 && !/\s/.test(text[at - 1]) ? ` ${token}` : token;
}

/** The image files a paste carries. */
export function pastedImages(data: DataTransfer | null): File[] {
  return Array.from(data?.files ?? []).filter((file) => file.type.startsWith("image/"));
}

function readDataUrl(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("unreadable"));
    reader.readAsDataURL(file);
  });
}

/**
 * A pasted image as the `data:` URL the core takes: as it is when it is a PNG, JPEG, GIF or WebP,
 * else (a TIFF…) drawn again as a PNG. Fails for what the window can't draw, and for one over the
 * size a card takes.
 */
export async function imageData(file: Blob): Promise<string> {
  let url = await readDataUrl(file);
  if (!KEPT_TYPES.includes(file.type)) {
    const picture = new Image();
    picture.src = url;
    await picture.decode();
    const canvas = document.createElement("canvas");
    canvas.width = picture.naturalWidth;
    canvas.height = picture.naturalHeight;
    canvas.getContext("2d")?.drawImage(picture, 0, 0);
    url = canvas.toDataURL("image/png");
  }
  // Base64: four characters for three bytes.
  const bytes = (url.length - url.indexOf(",") - 1) * 0.75;
  if (bytes > IMAGE_MB_MAX * 1024 * 1024) throw new Error("too large");
  return url;
}

/**
 * The saved images of `card()` as its note's strip draws them, in the card's order: each asked of
 * the core once and held while the card keeps it.
 */
export function useCardImages(card: () => Card | null): ComputedRef<Shot[]> {
  const loaded = ref(new Map<string, string>());
  const asked = new Set<string>();
  const keyOf = (id: string, image: Card["images"][number]): string => `${id} ${image.n} ${image.kind}`;

  watch(
    () => {
      const shown = card();
      return shown ? shown.images.map((image) => keyOf(shown.id, image)).join("\n") : "";
    },
    () => {
      const shown = card();
      const keys = new Set(shown ? shown.images.map((image) => keyOf(shown.id, image)) : []);
      // No longer the card's: a number given again later is another picture.
      for (const key of [...loaded.value.keys()]) if (!keys.has(key)) loaded.value.delete(key);
      for (const key of [...asked]) if (!keys.has(key)) asked.delete(key);
      if (!shown) return;

      for (const image of shown.images) {
        const key = keyOf(shown.id, image);
        if (asked.has(key)) continue;
        asked.add(key);
        void api
          .boardImage(shown.id, image.n)
          .then((src) => {
            if (asked.has(key)) loaded.value.set(key, src);
          })
          .catch(() => asked.delete(key));
      }
    },
    { immediate: true },
  );

  return computed(() => {
    const shown = card();
    return shown ? shown.images.map((image) => ({ n: image.n, src: loaded.value.get(keyOf(shown.id, image)) ?? null })) : [];
  });
}
