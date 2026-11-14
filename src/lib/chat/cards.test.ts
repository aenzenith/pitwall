import { describe, expect, it } from "vitest";

import { alwaysOffer } from "./cards";
import type { CanUseToolRequest, PermissionUpdate } from "./wire";

const request = (suggestions: PermissionUpdate[] | undefined, more: Partial<CanUseToolRequest> = {}): CanUseToolRequest => ({
  subtype: "can_use_tool",
  tool_name: "Bash",
  input: { command: "git status --short" },
  permission_suggestions: suggestions,
  ...more,
});

const rule = (content: string, behavior: "allow" | "deny" | "ask" = "allow"): PermissionUpdate => ({ type: "addRules", rules: [{ toolName: "Bash", ruleContent: content }], behavior, destination: "localSettings" });
const mode = (name: string): PermissionUpdate => ({ type: "setMode", mode: name, destination: "session" });

// "Yes, and don't ask again" writes into a settings file through the core: what the card names
// is what lands there, silently and for every later session.
describe("a permission card's always offer", () => {
  it("names only the CLI's own suggestions that allow, never one that denies or asks", () => {
    const offer = alwaysOffer(request([rule("git status:*"), rule("rm:*", "deny"), rule("curl:*", "ask"), { type: "removeRules", rules: [{ toolName: "Bash" }], behavior: "allow", destination: "localSettings" }]));
    expect(offer?.suggestions).toEqual([0]);
    expect(offer?.rules).toEqual(["Bash(git status:*)"]);
  });

  it("is none when the CLI says not to offer one, or suggests nothing", () => {
    expect(alwaysOffer(request([rule("git status:*")], { suppress_always_allow_rule: true }))).toBeNull();
    expect(alwaysOffer(request([]))).toBeNull();
    expect(alwaysOffer(request(undefined))).toBeNull();
  });

  it("keeps a suggested mode to this session: never a settings file's default mode", () => {
    const offer = alwaysOffer(request([mode("acceptEdits")], { tool_name: "Write" }));
    expect(offer?.places).toEqual(["session"]);
    expect(offer?.initial).toBe("session");
    expect(offer?.suggestions).toEqual([0]);
  });

  it("leaves a mode out when rules go to the place picked", () => {
    const offer = alwaysOffer(request([mode("acceptEdits"), rule("npm test:*")]));
    expect(offer?.suggestions).toEqual([1]);
    expect(offer?.places).toContain("userSettings");
  });

  it("never offers bypassing permissions", () => {
    expect(alwaysOffer(request([mode("bypassPermissions")]))).toBeNull();
  });
});
