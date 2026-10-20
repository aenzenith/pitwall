import { describe, expect, it } from "vitest";
import { nextTick, ref } from "vue";

import { useLinkDialog, type LinksChange } from "./linkDialog";
import type { ProjectLink } from "./types";

describe("the link dialog", () => {
  it("writes into the project it was opened for only, however the selection moves meanwhile", async () => {
    const shop: ProjectLink[] = [
      { name: "Docs", url: "https://docs.shop.test" },
      { name: "Staging", url: "https://staging.shop.test" },
    ];
    const blog: ProjectLink[] = [
      { name: "Admin", url: "https://blog.test/admin" },
      { name: "Production", url: "https://blog.example.com" },
    ];
    const links: Record<string, ProjectLink[]> = { "/p/shop": shop, "/p/blog": blog };
    const shown = ref("/p/shop");
    const { dialog, open, save, remove } = useLinkDialog(
      () => shown.value,
      (path: string, change: LinksChange) => (links[path] = change(links[path])),
    );
    const staging2 = { name: "Staging", url: "https://staging2.shop.test" };

    // Shop's second link open, then blog shown by itself (shop left a filter, a notification's
    // project came up): the dialog closes, and Save and Delete find nothing to write.
    for (const press of [() => save(staging2), remove]) {
      shown.value = "/p/shop";
      await nextTick();
      open(1);
      shown.value = "/p/blog";
      await nextTick();
      expect(dialog.value).toBeNull();
      press();
    }
    // Pressed before the dialog heard of it: into shop.
    shown.value = "/p/shop";
    await nextTick();
    open(1);
    shown.value = "/p/blog";
    save(staging2);

    expect(links["/p/blog"]).toBe(blog);
    expect(links["/p/shop"][1]).toEqual(staging2);
  });
});
