import { ref, watch } from "vue";

import { ago } from "./format";
import { t, type Key } from "./i18n";
import { api, deps } from "./store";
import type { DepAdvisory, DepCheck, DepMigrations, DepPackage, DepReport, DepRuntime, DepScan } from "./types";

/**
 * The Garage page: what a project's state is, what its row and its topics' lines say,
 * and the installs, migrations and scans asked from it. The actions' state lives here, not in the page: an install
 * keeps its spinner while another page is shown.
 */

/* ---------- names ---------- */

// Proper names, the same in every language, so not in the catalogs. An id the core sends that
// isn't here shows as itself.

/** A language, by its ecosystem's id; in the order lists of languages break their ties by. */
const ECOSYSTEM_NAMES = new Map([
  ["npm", "JavaScript"],
  ["composer", "PHP"],
  ["python", "Python"],
  ["ruby", "Ruby"],
  ["dotnet", ".NET"],
  ["java", "Java"],
  ["go", "Go"],
  ["rust", "Rust"],
  ["elixir", "Elixir"],
  ["dart", "Dart"],
  ["cpp", "C++"],
]);

/** A package manager, spelled as its own command or brand is. */
const MANAGER_NAMES = new Map([
  ["npm", "npm"],
  ["pnpm", "pnpm"],
  ["yarn", "yarn"],
  ["bun", "bun"],
  ["composer", "composer"],
  ["pip", "pip"],
  ["poetry", "poetry"],
  ["uv", "uv"],
  ["pipenv", "pipenv"],
  ["bundler", "bundler"],
  ["mix", "mix"],
  ["nuget", "NuGet"],
  ["maven", "Maven"],
  ["gradle", "Gradle"],
  ["go", "go"],
  ["cargo", "Cargo"],
  ["pub", "pub"],
  ["vcpkg", "vcpkg"],
  ["conan", "Conan"],
]);

const RUNTIME_NAMES = new Map([
  ["node", "Node"],
  ["php", "PHP"],
  ["python", "Python"],
  ["ruby", "Ruby"],
  ["elixir", "Elixir"],
  ["dotnet", ".NET"],
  ["java", "Java"],
  ["go", "Go"],
  ["rust", "Rust"],
  ["dart", "Dart"],
]);

/** A migration tool. */
const TOOL_NAMES = new Map([
  ["laravel", "Laravel"],
  ["doctrine", "Doctrine"],
  ["django", "Django"],
  ["alembic", "Alembic"],
  ["rails", "Rails"],
  ["ecto", "Ecto"],
  ["efcore", "EF Core"],
  ["flyway", "Flyway"],
  ["liquibase", "Liquibase"],
  ["prisma", "Prisma"],
  ["knex", "Knex"],
  ["sequelize", "Sequelize"],
  ["typeorm", "TypeORM"],
  ["adonis", "AdonisJS"],
  ["drizzle", "Drizzle"],
  ["sqlx", "SQLx"],
  ["diesel", "Diesel"],
  ["goose", "goose"],
]);

/** The ecosystem a runtime belongs to, where its id isn't the ecosystem's own (`python`, `ruby`, …). */
const RUNTIME_ECOSYSTEMS = new Map([
  ["node", "npm"],
  ["php", "composer"],
]);

function named(names: Map<string, string>, id: string): string {
  return names.get(id.toLowerCase()) ?? id;
}

/** `JavaScript`, `PHP`, `.NET`: the language of an ecosystem. */
export function ecosystemName(id: string): string {
  return named(ECOSYSTEM_NAMES, id);
}

/** `pnpm`, `uv`, `NuGet`. */
export function managerName(id: string): string {
  return named(MANAGER_NAMES, id);
}

/** `Node`, `PHP`, `.NET`. */
export function runtimeName(id: string): string {
  return named(RUNTIME_NAMES, id);
}

/** `Laravel`, `EF Core`. */
export function toolName(id: string): string {
  return named(TOOL_NAMES, id);
}

/** The known ecosystems in their own order, for lists of languages; unknown ones come after. */
function ecosystemOrder(id: string): number {
  const at = [...ECOSYSTEM_NAMES.keys()].indexOf(id);
  return at < 0 ? ECOSYSTEM_NAMES.size : at;
}

/* ---------- state ---------- */

/** No ecosystem hidden: every check and runtime counts (the sidebar's count). */
const NONE: readonly string[] = [];

/**
 * The order a project's managers and runtimes show (and install) in. The core sends them by
 * ecosystem id; here JavaScript goes after the project's other languages, where it is the
 * assets' toolchain beside the backend's: `composer`, `npm`; `uv`, `npm`; `PHP`, then `Node`.
 */
function inOrder<T>(list: readonly T[], ecosystem: (item: T) => string): T[] {
  return [...list].sort((a, b) => Number(ecosystem(a) === "npm") - Number(ecosystem(b) === "npm"));
}

function runtimeEcosystem(runtime: DepRuntime): string {
  return named(RUNTIME_ECOSYSTEMS, runtime.name);
}

/** The checks the language filter leaves, in the order they show: `hidden` are the ecosystems turned off. */
export function shownChecks(report: DepReport, hidden: readonly string[] = NONE): DepCheck[] {
  return inOrder(
    report.checks.filter((check) => !hidden.includes(check.ecosystem)),
    (check) => check.ecosystem,
  );
}

export function shownRuntimes(report: DepReport, hidden: readonly string[] = NONE): DepRuntime[] {
  return inOrder(
    report.runtimes.filter((runtime) => !hidden.includes(runtimeEcosystem(runtime))),
    runtimeEcosystem,
  );
}

/** Packages differ from the lock file and the manager is there to install them. */
export function needsInstall(check: DepCheck): boolean {
  return check.state === "install" && check.tool === "ok";
}

/** An install Pitwall can run: the only ones a button, the row's or "Install what's needed", starts. */
export function canInstall(check: DepCheck): boolean {
  return needsInstall(check) && check.installCommand !== null;
}

/** A scan Pitwall can run: the manager has an `outdated`/`audit` and is on this machine. */
export function canScan(check: DepCheck): boolean {
  return check.canScan && check.tool === "ok";
}

/** `install`: a check says install; else `mismatch`: a runtime doesn't satisfy what is asked;
 * else `ok` (a project whose checks are all unknown is ok too, shown as not watched). */
export type DepState = "install" | "mismatch" | "ok";

/** The project's state over the shown ecosystems' checks and runtimes. */
export function depState(report: DepReport, hidden: readonly string[] = NONE): DepState {
  if (shownChecks(report, hidden).some(needsInstall)) return "install";
  if (shownRuntimes(report, hidden).some((runtime) => runtime.ok === false)) return "mismatch";
  return "ok";
}

/** How many migrations the database says are still to run, over every migration tool (none
 * before a read; kept from the last good one when a later read fails). */
export function pendingMigrations(report: DepReport): number {
  return report.migrations.reduce((sum, migrations) => sum + migrations.pending.length, 0);
}

/** The shown checks whose manager isn't on this machine. */
export function missingTools(report: DepReport, hidden: readonly string[] = NONE): DepCheck[] {
  return shownChecks(report, hidden).filter((check) => check.tool === "missing");
}

/** Why a project needs you, most pressing first: the order inside the "Attention" group. */
export const ATTENTION = { install: 0, mismatch: 1, migration: 2, missingTool: 3, none: 4 } as const;

export function attentionRank(report: DepReport, hidden: readonly string[] = NONE): number {
  const state = depState(report, hidden);
  if (state === "install") return ATTENTION.install;
  if (state === "mismatch") return ATTENTION.mismatch;
  if (pendingMigrations(report) > 0) return ATTENTION.migration;
  if (missingTools(report, hidden).length) return ATTENTION.missingTool;
  return ATTENTION.none;
}

/**
 * Something to do before the project runs as its code expects: packages to install, a runtime
 * to switch, migrations pending in the database, a package manager to put on this machine. These
 * get the yellow mark, the "Attention" group and the sidebar's count; a database that couldn't be
 * read isn't one of them.
 */
export function needsAttention(report: DepReport, hidden: readonly string[] = NONE): boolean {
  return attentionRank(report, hidden) !== ATTENTION.none;
}

/** The languages among these reports, each with how many projects use it: the most used first. */
export function languages(reports: readonly DepReport[]): Array<{ id: string; count: number }> {
  const counts = new Map<string, number>();
  for (const report of reports) {
    for (const id of new Set(report.checks.map((check) => check.ecosystem))) counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  return [...counts].map(([id, count]) => ({ id, count })).sort((a, b) => b.count - a.count || ecosystemOrder(a.id) - ecosystemOrder(b.id));
}

/** What the search looks in besides the project itself: its languages, package managers,
 * framework and migration tools, in lower case. */
export function searchTerms(report: DepReport): string[] {
  const terms = [
    ...report.checks.flatMap((check) => [ecosystemName(check.ecosystem), check.manager, managerName(check.manager)]),
    ...report.runtimes.map((runtime) => runtimeName(runtime.name)),
    ...report.migrations.map((migrations) => toolName(migrations.tool)),
  ];
  if (report.framework) terms.push(report.framework.name);
  return [...new Set(terms.map((term) => term.toLowerCase()))];
}

/* ---------- actions ---------- */

/** The least an action's spinner shows, so one the core answers at once still shows. */
const MIN_SPIN_MS = 400;

async function minSpin(started: number): Promise<void> {
  const rest = MIN_SPIN_MS - (Date.now() - started);
  if (rest > 0) await new Promise((resolve) => window.setTimeout(resolve, rest));
}

/** A project's ecosystem, or its migration tool. */
function keyOf(path: string, id: string): string {
  return `${path}\n${id}`;
}

const MIGRATE_JOBS = "deps:migrate:";

/** The output job of an ecosystem's install, and of a migration tool's migrate. */
export function installJob(ecosystem: string): string {
  return `deps:${ecosystem}`;
}

export function migrateJob(tool: string): string {
  return `${MIGRATE_JOBS}${tool}`;
}

/** Installs asked for and not settled yet (the call, then the core's `installing`). */
const installAsked = ref<Record<string, true>>({});
/** Installs that ended with the packages still differing: when, and what the core said. */
const installFailed = ref<Record<string, { at: number; error: string | null }>>({});
/** Migrations asked for and not answered yet, by project and tool; and why the last one failed. */
const migrateAsked = ref<Record<string, true>>({});
const migrateFailed = ref<Record<string, string>>({});
/** Scans asked for and not answered yet. */
const scanAsked = ref<Record<string, true>>({});
/** "Install what's needed" is going through its list. */
export const installingAll = ref(false);
/** This page's jobs that ran this session, by project: the output panel shows their tabs. */
export const depJobs = ref<Record<string, string[]>>({});

function noteJob(path: string, job: string): void {
  const jobs = depJobs.value[path] ?? [];
  if (!jobs.includes(job)) depJobs.value = { ...depJobs.value, [path]: [...jobs, job] };
}

function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  const rest = { ...record };
  delete rest[key];
  return rest;
}

export function isInstalling(report: DepReport, ecosystem: string): boolean {
  return report.installing === ecosystem || !!installAsked.value[keyOf(report.path, ecosystem)];
}

export function anyInstalling(report: DepReport): boolean {
  return report.installing !== null || report.checks.some((check) => !!installAsked.value[keyOf(report.path, check.ecosystem)]);
}

/** The last install of this ecosystem ended, and its packages still differ. */
export function installFailure(report: DepReport, ecosystem: string): { at: number; error: string | null } | null {
  const failure = installFailed.value[keyOf(report.path, ecosystem)];
  const check = report.checks.find((c) => c.ecosystem === ecosystem);
  return failure && check?.state === "install" && !isInstalling(report, ecosystem) ? failure : null;
}

export function isMigrating(path: string, tool: string): boolean {
  return !!migrateAsked.value[keyOf(path, tool)];
}

/** What the core said when this tool's last migrate failed; empty when it didn't. */
export function migrateFailure(path: string, tool: string): string {
  return migrateFailed.value[keyOf(path, tool)] ?? "";
}

/** This tool's migrate ran this session: its output has a tab. */
export function ranMigrate(path: string, tool: string): boolean {
  return (depJobs.value[path] ?? []).includes(migrateJob(tool));
}

export function isScanning(report: DepReport, ecosystem: string): boolean {
  return !!report.scans[ecosystem]?.running || !!scanAsked.value[keyOf(report.path, ecosystem)];
}

function reportOf(path: string): DepReport | undefined {
  return deps.value?.find((report) => report.path === path);
}

/** Resolves once the project runs no install (at once, if it runs none). */
function settled(path: string): Promise<void> {
  if (!reportOf(path)?.installing) return Promise.resolve();
  return new Promise((resolve) => {
    const stop = watch(
      () => reportOf(path)?.installing ?? null,
      (installing) => {
        if (installing !== null) return;
        stop();
        resolve();
      },
    );
  });
}

// The core's `installing` coming and going: a tab for its output, and an install that ended with
// its packages still differing is a failed one. Back in order, or installing again, it isn't.
let wasInstalling = new Map<string, string | null>();
watch(deps, (reports) => {
  let failed = installFailed.value;
  const now = new Map<string, string | null>();
  for (const report of reports ?? []) {
    const was = wasInstalling.get(report.path) ?? null;
    if (report.installing) noteJob(report.path, installJob(report.installing));
    for (const check of report.checks) {
      const key = keyOf(report.path, check.ecosystem);
      if (was === check.ecosystem && report.installing !== check.ecosystem && check.state === "install") {
        failed = { ...failed, [key]: { at: Date.now(), error: failed[key]?.error ?? null } };
      } else if (key in failed && (check.state !== "install" || report.installing === check.ecosystem)) {
        failed = without(failed, key);
      }
    }
    now.set(report.path, report.installing);
  }
  wasInstalling = now;
  if (failed !== installFailed.value) installFailed.value = failed;
});

/** One ecosystem's install; resolves once it has ended (the core's `installing` cleared). */
export async function install(path: string, ecosystem: string): Promise<void> {
  const key = keyOf(path, ecosystem);
  if (installAsked.value[key]) return;
  installAsked.value = { ...installAsked.value, [key]: true };
  installFailed.value = without(installFailed.value, key);
  noteJob(path, installJob(ecosystem));
  const started = Date.now();
  let error: string | null = null;
  try {
    await api.depsInstall(path, ecosystem);
  } catch (reason) {
    error = String(reason);
  }
  // The spinner's least time is also the core's to say `installing`, if the call came back at
  // once; then the install's end is waited for.
  await minSpin(started);
  if (error === null) await settled(path);
  installAsked.value = without(installAsked.value, key);
  // Ended and still differing (or refused): failed, with the core's word if it gave one.
  const check = reportOf(path)?.checks.find((c) => c.ecosystem === ecosystem);
  if (error !== null || (check?.state === "install" && reportOf(path)?.installing !== ecosystem)) {
    installFailed.value = { ...installFailed.value, [key]: { at: Date.now(), error } };
  }
}

/** A project's installs, one after another. */
export async function installEach(path: string, ecosystems: readonly string[]): Promise<void> {
  for (const ecosystem of ecosystems) await install(path, ecosystem);
}

/** Installs, one after another within a project, the projects side by side. */
export async function installAll(targets: Array<{ path: string; ecosystem: string }>): Promise<void> {
  if (installingAll.value || !targets.length) return;
  installingAll.value = true;
  const byProject = new Map<string, string[]>();
  for (const target of targets) byProject.set(target.path, [...(byProject.get(target.path) ?? []), target.ecosystem]);
  await Promise.all([...byProject].map(([path, ecosystems]) => installEach(path, ecosystems)));
  installingAll.value = false;
}

/** One migration tool's migrate (`php artisan migrate`, `python manage.py migrate`, …). */
export async function migrate(path: string, tool: string): Promise<void> {
  const key = keyOf(path, tool);
  if (migrateAsked.value[key]) return;
  migrateAsked.value = { ...migrateAsked.value, [key]: true };
  migrateFailed.value = without(migrateFailed.value, key);
  noteJob(path, migrateJob(tool));
  const started = Date.now();
  try {
    await api.depsMigrate(path, tool);
  } catch (reason) {
    migrateFailed.value = { ...migrateFailed.value, [key]: String(reason) };
  }
  await minSpin(started);
  migrateAsked.value = without(migrateAsked.value, key);
}

/** `outdated` and `audit` for each ecosystem given, one after another (network). */
export async function scan(path: string, ecosystems: readonly string[]): Promise<void> {
  for (const ecosystem of ecosystems) {
    const key = keyOf(path, ecosystem);
    if (scanAsked.value[key]) continue;
    scanAsked.value = { ...scanAsked.value, [key]: true };
    const started = Date.now();
    await api.depsScan(path, ecosystem).catch(() => undefined);
    await minSpin(started);
    scanAsked.value = without(scanAsked.value, key);
  }
}

/* ---------- text ---------- */

/** An output tab's name for one of this page's jobs, the command it ran (`uv sync`,
 * `php artisan migrate`); null for any other job. */
export function depJobName(path: string, job: string): string | null {
  if (!job.startsWith("deps:")) return null;
  const report = reportOf(path);
  if (job.startsWith(MIGRATE_JOBS)) {
    const tool = job.slice(MIGRATE_JOBS.length);
    return report?.migrations.find((migrations) => migrations.tool === tool)?.command ?? t("deps.job.migrate");
  }
  const ecosystem = job.slice("deps:".length);
  const check = report?.checks.find((c) => c.ecosystem === ecosystem);
  return check?.installCommand ?? managerName(check?.manager ?? ecosystem);
}

/** `PHP ^8.4`, `Node >=22`: what it asks, else what is active. */
export function runtimeText(runtime: DepRuntime): string {
  return `${runtimeName(runtime.name)} ${runtime.required ?? runtime.active ?? t("deps.runtime.unknown")}`;
}

const SCAN_ERRORS: Record<NonNullable<DepScan["error"]>, { short: Key; long: Key }> = {
  offline: { short: "deps.scan.offline", long: "deps.scan.offlineBody" },
  timeout: { short: "deps.scan.timeout", long: "deps.scan.timeoutBody" },
  failed: { short: "deps.scan.failed", long: "deps.scan.failedBody" },
};

/** Why a scan got no counts; an error the page doesn't know reads as a failed one. */
function scanError(error: DepScan["error"]): { short: Key; long: Key } {
  return SCAN_ERRORS[error ?? "failed"] ?? SCAN_ERRORS.failed;
}

const DB_ERRORS: Record<NonNullable<DepMigrations["error"]>, Key> = {
  unreachable: "deps.db.unreachable",
  noTable: "deps.db.noTable",
  timeout: "deps.db.timeout",
  failed: "deps.db.failed",
  toolMissing: "deps.db.toolMissing",
};

/** Why the last read of the migrations got no list; empty when it got one. */
export function dbErrorText(migrations: DepMigrations): string {
  return migrations.error ? t(DB_ERRORS[migrations.error] ?? DB_ERRORS.failed) : "";
}

/** `mysql · web_sehir360`: where the migrations would go; empty when the core doesn't know. */
export function databaseText(migrations: DepMigrations): string {
  const database = migrations.database;
  return database ? [database.connection, database.name].filter(Boolean).join(" · ") : "";
}

/* ---------- the list ---------- */

/** A text of a project's row: how loud, a spinner before it, and what a narrow column cuts. */
export type HeadText = { text: string; tone: "plain" | "dim" | "faint"; spin: boolean; title: string };

/** The last scan, over the shown ecosystems: `5 updates · no vulnerabilities`; when it ran is in
 * the tooltip. */
export function scanSummary(report: DepReport, hidden: readonly string[], now: number): HeadText {
  const ecosystems = shownChecks(report, hidden).map((check) => check.ecosystem);
  if (ecosystems.some((ecosystem) => isScanning(report, ecosystem))) return { text: t("deps.scan.running"), tone: "dim", spin: true, title: "" };
  const scans = ecosystems.map((ecosystem) => report.scans[ecosystem]).filter((s): s is DepScan => !!s);
  if (!scans.length) return { text: t("deps.scan.never"), tone: "faint", spin: false, title: "" };
  const when = ago(Math.max(...scans.map((s) => s.at)), now);
  const counted = scans.filter((s) => s.outdated !== null);
  if (!counted.length) {
    const error = scanError(scans.find((s) => s.error)?.error ?? null);
    return { text: t(error.short), tone: "dim", spin: false, title: `${t(error.long)} · ${when}` };
  }
  const outdated = counted.reduce((sum, s) => sum + (s.outdated ?? 0), 0);
  const audited = scans.filter((s) => s.vulnerable !== null);
  const vulnerable = audited.reduce((sum, s) => sum + (s.vulnerable ?? 0), 0);
  const parts = [outdated ? t("deps.scan.outdated", { count: outdated }) : t("deps.scan.upToDate")];
  if (audited.length) parts.push(vulnerable ? t("deps.scan.vulnerable", { count: vulnerable }) : t("deps.scan.noVulnerable"));
  const text = parts.join(" · ");
  return { text, tone: "plain", spin: false, title: `${text} · ${when}` };
}

/** What a scan's details show first: the outdated packages, or the advisories. */
export type ScanView = "updates" | "vulns";

/** A piece of what a scan found. `open`: it is a count, and a click shows what it counts. */
export type ScanPart = { text: string; open: ScanView | null };

/** One manager's last scan, its line under "Updates": what it found (`parts`: piece by piece),
 * when; `offline`: it had no network. */
export type ScanLine = { text: string; parts: ScanPart[]; when: string; tone: "plain" | "dim" | "faint"; spin: boolean; error: boolean; offline: boolean };

export function scanLine(report: DepReport, ecosystem: string, now: number): ScanLine {
  const line = (text: string, tone: ScanLine["tone"], more: Partial<ScanLine> = {}): ScanLine => ({ text, parts: [], tone, when: "", spin: false, error: false, offline: false, ...more });
  if (isScanning(report, ecosystem)) return line(t("deps.scan.running"), "dim", { spin: true });
  const last = report.scans[ecosystem];
  if (!last) return line(t("deps.scan.never"), "faint");
  const when = ago(last.at, now);
  if (last.outdated === null) return line(t(scanError(last.error).long), "dim", { when, error: true, offline: last.error === "offline" });
  const parts: ScanPart[] = [last.outdated ? { text: t("deps.scan.outdated", { count: last.outdated }), open: "updates" } : { text: t("deps.scan.upToDate"), open: null }];
  if (last.vulnerable !== null) parts.push(last.vulnerable ? { text: t("deps.scan.vulnerable", { count: last.vulnerable }), open: "vulns" } : { text: t("deps.scan.noVulnerable"), open: null });
  return line(parts.map((part) => part.text).join(" · "), "plain", { when, parts });
}

/** Why a scan gave no list, in a sentence. */
export function scanErrorText(error: DepScan["error"]): string {
  return t(scanError(error).long);
}

/* ---------- a scan's details ---------- */

/** How far the newest version is from the installed one. */
export type Bump = "major" | "minor" | "patch";

const BUMPS: Record<Bump, Key> = { major: "deps.scan.kind.major", minor: "deps.scan.kind.minor", patch: "deps.scan.kind.patch" };

/** The numbers a version starts with (`v13.31.0`: 13, 31, 0); null where it has none. */
function numbers(version: string | null): number[] | null {
  const found = /^\D*(\d+)(?:\.(\d+))?(?:\.(\d+))?/.exec(version?.trim() ?? "");
  return found ? [Number(found[1]), Number(found[2] ?? 0), Number(found[3] ?? 0)] : null;
}

/** `major`, `minor` or `patch`: the first number the newest version raises; null when it isn't
 * ahead of the installed one, or the two can't be compared. */
export function bump(installed: string | null, latest: string | null): Bump | null {
  const from = numbers(installed);
  const to = numbers(latest);
  if (!from || !to) return null;
  const at = [0, 1, 2].find((i) => from[i] !== to[i]);
  return at === undefined || to[at] < from[at] ? null : (["major", "minor", "patch"] as const)[at];
}

/** `major`, `minor`, `patch`, in the page's language; a dash where there is no telling. */
export function bumpText(installed: string | null, latest: string | null): string {
  const kind = bump(installed, latest);
  return kind ? t(BUMPS[kind]) : "—";
}

const SEVERITIES: Record<NonNullable<DepAdvisory["severity"]>, Key> = {
  critical: "deps.scan.severity.critical",
  high: "deps.scan.severity.high",
  moderate: "deps.scan.severity.moderate",
  low: "deps.scan.severity.low",
  info: "deps.scan.severity.info",
};

/** `critical`, `high`, …, in the page's language; a dash where the audit told none. */
export function severityText(severity: DepAdvisory["severity"]): string {
  return severity && SEVERITIES[severity] ? t(SEVERITIES[severity]) : "—";
}

/** What fixes an advisory: as the audit wrote it, else whether the manager has a fix at all. */
export function fixText(advisory: DepAdvisory): string {
  if (advisory.fix) return advisory.fix;
  if (advisory.fixable === null) return "—";
  return t(advisory.fixable ? "deps.scan.fix.available" : "deps.scan.fix.none");
}

/** What an advisory's link reads: its id where the address ends in one (`GHSA-…`, `CVE-…`),
 * else the address's host. */
export function advisoryLabel(url: string): string {
  const last = url.split(/[/?#]/).filter(Boolean).pop() ?? "";
  if (/^[A-Za-z]+-[\w-]+$/.test(last)) return last;
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/* ---------- the lines ---------- */

/** What a project's lines are about, in the order they show. */
export type DepTopic = "packages" | "database" | "runtime" | "updates";

const TOPIC_LABELS: Record<DepTopic, Key> = {
  packages: "deps.topic.packages",
  database: "deps.topic.database",
  runtime: "deps.topic.runtime",
  updates: "deps.topic.updates",
};

/**
 * What every line says, in the same columns whatever its topic: `name`, its tool, a proper name
 * (`mono`: as its command is spelled); `state`, as loud as `tone` (`todo`: something to do;
 * `bad`: a failed install, the page's only red); `note`: more about it, beside the state.
 * `todo`: something to do, so the project needs you. `spin`: it runs, or (`spinTitle`) its
 * database is read again.
 */
type LineText = {
  id: string;
  topic: DepTopic;
  name: string;
  mono: boolean;
  state: string;
  tone: "todo" | "bad" | "plain" | "dim" | "faint";
  spin: boolean;
  spinTitle: string;
  note: string;
  todo: boolean;
};

/**
 * One line of a project. `install`: a package manager with packages to install, `command` what
 * its button runs (null when Pitwall can't), `packages` what would change; `missing`: a package
 * manager that isn't on this machine; `migration`: a migration tool, `command` what the confirm
 * shows, `blocked`: the tool that would run it isn't on this machine, `readError`: why its list
 * is an older one, `ran`: its output has a tab, `failure`: what the core said when the last
 * migrate failed; `scan`: a manager's last scan, `scannable`: Pitwall can run one, `parts`: what
 * it found piece by piece, its counts to be pressed for what they count; `text`: nothing to
 * press.
 */
export type DepLine = LineText &
  (
    | { kind: "install"; ecosystem: string; command: string | null; installing: boolean; failed: boolean; packages: DepPackage[]; differing: number }
    | { kind: "missing" }
    | { kind: "migration"; tool: string; command: string; pending: string[]; migrating: boolean; blocked: boolean; readError: string; ran: boolean; failure: string }
    | { kind: "scan"; ecosystem: string; scannable: boolean; error: boolean; offline: boolean; parts: ScanPart[] }
    | { kind: "text" }
  );

function lineText(id: string, topic: DepTopic, name: string, state: string, more: Partial<LineText> = {}): LineText {
  return { id, topic, name, state, mono: true, tone: "dim", spin: false, spinTitle: "", note: "", todo: false, ...more };
}

/** A package manager's line: missing, to install (installing, failed), or with nothing to do. */
function packageLine(report: DepReport, check: DepCheck): DepLine {
  const text = (state: string, more: Partial<LineText> = {}): LineText => lineText(`packages:${check.ecosystem}`, "packages", managerName(check.manager), state, more);
  if (check.tool === "missing") return { ...text(t("deps.cell.missing"), { tone: "todo", todo: true, note: t("deps.todo.missingBody") }), kind: "missing" };
  if (check.state === "install") {
    const installing = isInstalling(report, check.ecosystem);
    const failed = !!installFailure(report, check.ecosystem);
    const state = installing ? t("deps.cell.installing") : failed ? t("deps.cell.failed") : check.differing ? t("deps.cell.differing", { count: check.differing }) : t("deps.cell.install");
    return {
      ...text(state, { tone: failed ? "bad" : "todo", todo: true, spin: installing }),
      kind: "install",
      ecosystem: check.ecosystem,
      command: check.installCommand,
      installing,
      failed,
      packages: check.packages,
      differing: check.differing,
    };
  }
  if (check.installCommand === null) return { ...text(t("deps.line.atBuild")), kind: "text" };
  if (check.state === "unknown") return { ...text(t("deps.line.noLock"), { tone: "faint", note: t("deps.unknown.body") }), kind: "text" };
  return { ...text(t("deps.line.locked")), kind: "text" };
}

/** A migration tool's line: pending migrations, a read in flight or failed, or nothing pending.
 * A tool whose database was never read, with nothing asked of it, has none yet. */
function databaseLines(report: DepReport): DepLine[] {
  return report.migrations.flatMap((found): DepLine[] => {
    const count = found.pending.length;
    const migrating = isMigrating(report.path, found.tool);
    const failure = migrateFailure(report.path, found.tool);
    const readError = dbErrorText(found);
    if (!count && !readError && !found.running && !found.at && !migrating && !failure) return [];
    const state = count ? t("deps.migrations", { count }) : readError || (found.running ? t("deps.db.reading") : t("deps.mig.none"));
    return [
      {
        ...lineText(`database:${found.tool}`, "database", toolName(found.tool), state, {
          mono: false,
          tone: count ? "todo" : "dim",
          todo: count > 0,
          spin: migrating || found.running,
          spinTitle: count && found.running && !migrating ? t("deps.db.reading") : "",
          // A read that failed says so in the state's place, with the line to itself.
          note: count || !readError ? databaseText(found) : "",
        }),
        kind: "migration",
        tool: found.tool,
        command: found.command,
        pending: found.pending,
        migrating,
        blocked: found.error === "toolMissing",
        readError: count ? readError : "",
        ran: ranMigrate(report.path, found.tool),
        failure,
      },
    ];
  });
}

/** A runtime's line: `needs ^8.5 · active 8.4.13`, `>=22 satisfied · active 22.19.0`, `active 23.6.1`. */
function runtimeLine(runtime: DepRuntime): DepLine {
  const text = (state: string, more: Partial<LineText> = {}): DepLine => ({ ...lineText(`runtime:${runtime.name}`, "runtime", runtimeName(runtime.name), state, { mono: false, ...more }), kind: "text" });
  const active = runtime.active ? t("deps.runtime.active", { version: runtime.active }) : "";
  if (runtime.ok === false) return text(t("deps.todo.needs", { version: runtime.required ?? "" }), { tone: "todo", todo: true, note: active || t("deps.runtime.unknown") });
  if (!runtime.required) return text(active || t("deps.runtime.unknown"));
  return text(runtime.ok ? t("deps.runtime.met", { version: runtime.required }) : runtime.required, { note: active });
}

/**
 * A project's lines over the shown ecosystems, topic by topic: its package managers, its
 * migration tools, its runtimes, then each manager's last scan (one that can't be scanned any
 * more keeps the scan it has).
 */
export function depLines(report: DepReport, hidden: readonly string[], now: number): DepLine[] {
  const checks = shownChecks(report, hidden);
  const scans = checks
    .filter((check) => canScan(check) || !!report.scans[check.ecosystem])
    .map((check): DepLine => {
      const found = scanLine(report, check.ecosystem, now);
      return {
        ...lineText(`updates:${check.ecosystem}`, "updates", managerName(check.manager), found.text, { tone: found.tone, spin: found.spin, note: found.when }),
        kind: "scan",
        ecosystem: check.ecosystem,
        scannable: canScan(check),
        error: found.error,
        offline: found.offline,
        parts: found.parts,
      };
    });
  return [...checks.map((check) => packageLine(report, check)), ...databaseLines(report), ...shownRuntimes(report, hidden).map(runtimeLine), ...scans];
}

/** A topic's lines, under its name (`Packages`, `Database`, `Runtime`, `Updates`). `mark`: as a
 * project's, for the topic alone: an install runs, one failed, something to do, nothing. */
export type DepTopicLines = { topic: DepTopic; label: string; mark: "busy" | "bad" | "warn" | "ok"; lines: DepLine[] };

function topicMark(lines: readonly DepLine[]): DepTopicLines["mark"] {
  if (lines.some((line) => line.kind === "install" && line.installing)) return "busy";
  if (lines.some((line) => line.tone === "bad")) return "bad";
  return lines.some((line) => line.todo) ? "warn" : "ok";
}

/** The lines topic by topic, in the order they show; a topic with no line has no entry. */
export function depTopics(lines: readonly DepLine[]): DepTopicLines[] {
  const topics = new Map<DepTopic, DepLine[]>();
  for (const line of lines) topics.set(line.topic, [...(topics.get(line.topic) ?? []), line]);
  return [...topics].map(([topic, found]) => ({ topic, label: t(TOPIC_LABELS[topic]), mark: topicMark(found), lines: found }));
}

/** What is to do on a line, in a few words: `npm to install`, `1 pending migration`, `Ruby needs 3.4`. */
function todoText(line: DepLine): string {
  if (line.kind === "migration") return line.state;
  return `${line.name} ${line.kind === "install" && !line.installing && !line.failed ? t("deps.cell.install") : line.state}`;
}

/**
 * Under a project's name in the list: what is to do, shortly (`npm to install · 1 pending
 * migration`); with nothing to do, its last scan; `not watched` where Pitwall knows no package
 * file. A narrow list cuts it, its tooltip keeps it.
 */
export function rowSummary(report: DepReport, hidden: readonly string[], now: number): HeadText {
  if (!report.checks.length && !report.runtimes.length && !report.migrations.length) return { text: t("deps.cell.unknown"), tone: "faint", spin: false, title: t("deps.untracked.title") };
  const todos = depLines(report, hidden, now).filter((line) => line.todo);
  if (!todos.length) {
    // Nothing to do: its last scan, a step quieter than what is to do.
    const last = scanSummary(report, hidden, now);
    return { ...last, tone: last.tone === "plain" ? "dim" : last.tone };
  }
  const text = todos.map(todoText).join(" · ");
  return { text, tone: "plain", spin: false, title: text };
}
