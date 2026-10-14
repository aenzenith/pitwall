// The images of a card's note: `[Image #n]` in its text, as Claude Code writes one in its prompt,
// and the picture itself, pasted from the clipboard. The core keeps the files; the window holds a
// picture only as a `data:` URL, to draw it and to hand a new one over.

import { computed, ref, watch, type ComputedRef, type Ref } from "vue";

import { t } from "./i18n";
import { api } from "./store";
import type { Card, NewImage } from "./types";

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
 * The images pasted into a note being written (the card dialog's, the quick switcher's): a paste
 * that carries one puts `[Image #n]` at the caret of `field()` at once, as Claude Code's prompt
 * does, and the picture once it is read. An image stays while the note names it. `say`: told why
 * one was refused (too many, too large, not a picture), and "" when the next one is taken.
 * `saved`: the images the card holds already (an edit), which the note's strip draws with them.
 */
export function useNoteImages(note: Ref<string>, field: () => HTMLTextAreaElement | null, say: (message: string) => void, saved: () => Shot[] = () => []) {
  /** Those pasted here (not saved yet; `src` null while one is read). */
  const pasted = ref<Shot[]>([]);
  /** Each pasted image being read: saving waits for them. */
  const reading: Promise<void>[] = [];
  /** The images the note names, in its order: what the card holds once saved. */
  const shots = computed(() =>
    imageNumbers(note.value)
      .map((n) => pasted.value.find((shot) => shot.n === n) ?? saved().find((shot) => shot.n === n))
      .filter((shot): shot is Shot => !!shot),
  );

  function refused(): void {
    say(t("board.dialog.imageRefused", { count: IMAGES_MAX, size: IMAGE_MB_MAX }));
  }

  /** Puts `file` into the note: its token at the caret at once, the picture once it is read. */
  function attach(file: File): void {
    say("");
    if (shots.value.length >= IMAGES_MAX) {
      refused();
      return;
    }

    const n = nextImageNumber(note.value, [...saved(), ...pasted.value].map((image) => image.n));
    pasted.value.push({ n, src: null });
    const target = field();
    if (target) {
      target.setRangeText(spaced(target.value, target.selectionStart, imageToken(n)), target.selectionStart, target.selectionEnd, "end");
      note.value = target.value;
    } else {
      note.value += spaced(note.value, note.value.length, imageToken(n));
    }

    reading.push(
      imageData(file)
        .then((src) => {
          const shot = pasted.value.find((shot) => shot.n === n);
          if (shot) shot.src = src;
        })
        .catch(() => {
          remove(n);
          refused();
        }),
    );
  }

  /** A paste that carries an image puts the image in; any other is the text it always was. */
  function onPaste(event: ClipboardEvent): void {
    const files = pastedImages(event.clipboardData);
    if (!files.length) return;
    event.preventDefault();
    for (const file of files) attach(file);
  }

  /** Takes image `n` out of the note; a saved one leaves the card when the card is saved. */
  function remove(n: number): void {
    note.value = withoutImage(note.value, n);
    pasted.value = pasted.value.filter((shot) => shot.n !== n);
  }

  /** Every pasted image is read, or was refused and is out of the note. */
  function settled(): Promise<unknown> {
    return Promise.all(reading);
  }

  /** The pasted images `text` still names, as the core takes them with the card. */
  function taken(text: string): NewImage[] {
    const named = imageNumbers(text);
    return pasted.value.flatMap((shot) => (shot.src && named.includes(shot.n) ? [{ n: shot.n, data: shot.src }] : []));
  }

  /** The note was saved or dropped: none of them is held any longer. */
  function clear(): void {
    pasted.value = [];
    reading.length = 0;
  }

  /** Lets go of each pasted image the note no longer names (its token deleted, the note emptied). */
  function forget(): void {
    const named = imageNumbers(note.value);
    pasted.value = pasted.value.filter((shot) => named.includes(shot.n));
    if (!pasted.value.length) reading.length = 0;
  }

  return { shots, onPaste, remove, settled, taken, clear, forget };
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
