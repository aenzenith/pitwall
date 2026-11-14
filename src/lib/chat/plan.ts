/**
 * Claude's plan (ExitPlanMode) on its own screen (PL5, the user's pick of 18 Nov): what the screen,
 * its bar over the composer and its split button share.
 */
import type { Key } from "../i18n";
import { modelOf } from "./commands";
import { peekChat } from "./store";
import type { PlanMode } from "./ui";

/** Whether the chat's model is known to have auto mode: before the models are listed, it isn't,
 * rather than a plan starting in a mode the CLI might refuse. */
export function planAutoOn(id: number): boolean {
  const chat = peekChat(id);
  const info = chat?.info ?? null;
  return modelOf(info, chat?.model ?? info?.model ?? null)?.supportsAutoMode === true;
}

/** The mode a plan starts in: a picked auto that the model hasn't goes back to accepting edits. */
export function planModeOf(id: number, mode: PlanMode): PlanMode {
  return mode === "auto" && !planAutoOn(id) ? "acceptEdits" : mode;
}

/** What starting in each mode is called under the screen's question. */
export const STARTS_IN: Record<PlanMode, Key> = {
  acceptEdits: "chatInput.plan.startsIn.acceptEdits",
  auto: "chatInput.plan.startsIn.auto",
  default: "chatInput.plan.startsIn.default",
};

/** The plan's own title: its first line when that is a top heading (`# …`). */
export function planTitle(plan: string): string {
  const first = plan.split("\n").find((line) => line.trim().length > 0) ?? "";
  const match = /^#\s+(.+?)\s*#*\s*$/.exec(first.trim());
  return match ? match[1] : "";
}
