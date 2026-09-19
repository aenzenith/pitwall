import * as path from 'path';
import * as vscode from 'vscode';

import { ownedProjects, type Favorite, type ProjectState, type Registry } from './registry';
import type { DevRunner, Target } from './runner';
import { targetFromFolder, targetFromPath } from './runner';

type Scope = 'local' | 'favorite' | 'remote';

export type ProjectNode = {
    kind: 'project';
    scope: Scope;
    state: ProjectState;
    favorite: boolean;
    /** Projeyi açık tutan başka pencere varsa onun kimliği. */
    windowId?: string;
    windowTitle?: string;
};

type GroupNode = {
    kind: 'group';
    id: string;
    label: string;
    detail: string;
};

export type Node = GroupNode | ProjectNode;

/**
 * Panel ağacı: bu pencerenin kökleri, favoriler ve açık diğer pencereler.
 */
export class DevTree implements vscode.TreeDataProvider<Node> {
    private readonly emitter = new vscode.EventEmitter<Node | undefined>();

    public readonly onDidChangeTreeData = this.emitter.event;

    public constructor(
        private readonly runner: DevRunner,
        private readonly registry: Registry,
    ) {}

    public refresh(): void {
        this.emitter.fire(undefined);
    }

    public dispose(): void {
        this.emitter.dispose();
    }

    /** Bu pencerede açık olan kök klasörler. */
    public localTargets(): Target[] {
        return (vscode.workspace.workspaceFolders ?? []).map(targetFromFolder);
    }

    /** Panelde görünen her satırın hedefi — favoriler dahil. */
    public allTargets(): Target[] {
        const targets = this.localTargets();
        const seen = new Set(targets.map((target) => target.path));

        for (const favorite of this.registry.readFavorites()) {
            if (!seen.has(favorite.path)) {
                targets.push(targetFromPath(favorite.path, favorite.name));
                seen.add(favorite.path);
            }
        }

        return targets;
    }

    public getTreeItem(node: Node): vscode.TreeItem {
        if (node.kind === 'group') {
            const item = new vscode.TreeItem(node.label, vscode.TreeItemCollapsibleState.Expanded);
            item.description = node.detail;
            item.contextValue = `group.${node.id}`;

            return item;
        }

        return this.projectItem(node);
    }

    public getChildren(node?: Node): Node[] {
        if (!node) {
            return this.groups();
        }

        if (node.kind === 'group') {
            return this.projectsOf(node.id);
        }

        return [];
    }

    /** Durum çubuğu rozeti için: bütün pencerelerde çalışan sayısı. */
    public runningTotals(): { here: number; everywhere: number } {
        const here = this.runner.runningPaths().length;
        const everywhere = this.registry
            .readWindows()
            .reduce((total, record) => total + ownedProjects(record).filter((project) => project.running).length, 0);

        return { here, everywhere };
    }

    /** Paneldeki bütün proje satırları — üç grubun tamamı. */
    public allNodes(): ProjectNode[] {
        return [...this.projectsOf('local'), ...this.projectsOf('favorite'), ...this.projectsOf('remote')];
    }

    private groups(): GroupNode[] {
        const groups: GroupNode[] = [];
        const local = this.projectsOf('local');
        const favorites = this.projectsOf('favorite');
        const remote = this.projectsOf('remote');

        if (local.length > 0) {
            groups.push({ kind: 'group', id: 'local', label: vscode.l10n.t('This window'), detail: summarize(local) });
        }

        if (favorites.length > 0) {
            groups.push({ kind: 'group', id: 'favorite', label: vscode.l10n.t('Favourites'), detail: summarize(favorites) });
        }

        if (remote.length > 0) {
            groups.push({ kind: 'group', id: 'remote', label: vscode.l10n.t('Other windows'), detail: summarize(remote) });
        }

        return groups;
    }

    private projectsOf(scope: string): ProjectNode[] {
        const favorites = this.registry.readFavorites();
        const isFavorite = (folderPath: string): boolean => favorites.some((item) => item.path === folderPath);
        const localPaths = new Set(this.localTargets().map((target) => target.path));
        const peers = this.registry.readPeers();

        if (scope === 'local') {
            return this.localTargets().map((target) => ({
                kind: 'project',
                scope: 'local',
                state: this.runner.stateOf(target),
                favorite: isFavorite(target.path),
            }));
        }

        if (scope === 'favorite') {
            return favorites
                .filter((favorite) => !localPaths.has(favorite.path))
                .map((favorite) => this.remoteOrClosed(favorite, peers, true));
        }

        const nodes: ProjectNode[] = [];

        for (const peer of peers) {
            for (const project of ownedProjects(peer)) {
                if (localPaths.has(project.folderPath) || isFavorite(project.folderPath)) {
                    continue;
                }

                nodes.push({
                    kind: 'project',
                    scope: 'remote',
                    state: project,
                    favorite: false,
                    windowId: peer.windowId,
                    windowTitle: peer.title,
                });
            }
        }

        return nodes;
    }

    /** Favori: başka pencerede açıksa oradaki durumu, değilse kapalı satır. */
    private remoteOrClosed(
        favorite: Favorite,
        peers: ReturnType<Registry['readPeers']>,
        isFavorite: boolean,
    ): ProjectNode {
        for (const peer of peers) {
            const match = ownedProjects(peer).find((project) => project.folderPath === favorite.path);

            if (match) {
                return {
                    kind: 'project',
                    scope: 'favorite',
                    state: match,
                    favorite: isFavorite,
                    windowId: peer.windowId,
                    windowTitle: peer.title,
                };
            }
        }

        const target = targetFromPath(favorite.path, favorite.name);

        return {
            kind: 'project',
            scope: 'favorite',
            state: this.runner.stateOf(target),
            favorite: isFavorite,
        };
    }

    private projectItem(node: ProjectNode): vscode.TreeItem {
        const { state } = node;
        const busy = this.runner.isBusy(state.folderPath);
        const item = new vscode.TreeItem(state.name, vscode.TreeItemCollapsibleState.None);

        item.description = this.describe(node, busy);
        item.tooltip = this.tooltip(node);
        item.iconPath = this.icon(node, busy);
        item.contextValue = this.contextValue(node);
        // Satıra tıklamak hiçbir şey başlatmaz: projenin penceresine götürür.
        item.command = {
            command: 'pitwall.focusWindow',
            title: vscode.l10n.t('Go to its window'),
            arguments: [node],
        };

        return item;
    }

    private describe(node: ProjectNode, busy: boolean): string {
        if (busy) {
            return vscode.l10n.t('restarting…');
        }

        // Sorun varsa satırda yazan odur; yoksa yalnız pencere etiketi kalır.
        return node.state.issue?.text ?? windowLabel(node);
    }

    private tooltip(node: ProjectNode): vscode.MarkdownString {
        const lines = [`**${node.state.name}**`, '', `\`${node.state.folderPath}\``];

        if (node.state.issue) {
            lines.push('', `**${node.state.issue.text}**`, '', vscode.l10n.t('Open the output for details.'));
        }

        if (node.state.url) {
            lines.push('', node.state.url);
        }

        if (node.windowTitle) {
            lines.push('', vscode.l10n.t('Window: {0}', node.windowTitle));
        } else if (node.scope === 'favorite') {
            lines.push('', vscode.l10n.t('Window closed — starting it runs the server from this window.'));
        }

        return new vscode.MarkdownString(lines.join('\n'));
    }

    private icon(node: ProjectNode, busy: boolean): vscode.ThemeIcon {
        if (busy) {
            return new vscode.ThemeIcon('sync~spin');
        }

        const issue = node.state.issue;

        if (issue?.kind === 'crashed') {
            return new vscode.ThemeIcon('error', new vscode.ThemeColor('testing.iconFailed'));
        }

        if (issue) {
            return new vscode.ThemeIcon('warning', new vscode.ThemeColor('list.warningForeground'));
        }

        if (node.state.running) {
            return new vscode.ThemeIcon('circle-filled', new vscode.ThemeColor('testing.iconPassed'));
        }

        // Favorinin penceresi hiç açık değil: ayrı ikon, satırda metne gerek kalmasın.
        if (node.scope === 'favorite' && !node.windowId) {
            return new vscode.ThemeIcon('circle-slash');
        }

        return new vscode.ThemeIcon('circle-outline');
    }

    /**
     * Satır içi düğmeleri seçen anahtar: `project.<kapsam>.<durum>.<fav|nofav>`.
     * Yıldız ikonu komuta bağlı olduğu için favori/değil iki ayrı komutla çizilir.
     */
    private contextValue(node: ProjectNode): string {
        const where = node.scope === 'remote' || (node.scope === 'favorite' && node.windowId) ? 'remote' : 'here';
        const state = node.state.running ? 'running' : 'idle';

        return `project.${where}.${state}.${node.favorite ? 'fav' : 'nofav'}`;
    }
}

/**
 * Pencere başlığı yalnız proje adından farklıysa yazılır.
 * Tek klasörlü pencerede başlık = klasör adıdır; yoksa satırda ad iki kez görünür.
 */
function windowLabel(node: ProjectNode): string {
    if (node.scope === 'local' || !node.windowTitle) {
        return '';
    }

    return node.windowTitle === node.state.name ? '' : node.windowTitle;
}

function summarize(nodes: ProjectNode[]): string {
    const running = nodes.filter((node) => node.state.running).length;

    return running > 0 ? vscode.l10n.t('{0} running', String(running)) : '';
}

export function favoriteOf(node: ProjectNode): Favorite {
    return { path: node.state.folderPath, name: node.state.name || path.basename(node.state.folderPath) };
}
