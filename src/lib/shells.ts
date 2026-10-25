import type { Key } from "./i18n";
import type { Shell } from "./types";

/** Each shell of the list (core/shells.rs) by its name as Windows gives it. */
export const SHELL_LABELS: Record<Shell, Key> = {
  cmd: "terminal.shell.cmd",
  powershell: "terminal.shell.powershell",
  pwsh: "terminal.shell.pwsh",
};

/** The mark a shell tab and its pane's head carry: PowerShell's own, else a prompt's. */
export function shellIcon(shell: Shell | null | undefined): "powershell" | "terminal" {
  return shell === "powershell" || shell === "pwsh" ? "powershell" : "terminal";
}
