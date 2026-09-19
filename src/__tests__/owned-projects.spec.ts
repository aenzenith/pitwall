import { describe, expect, it } from 'vitest';

import { ownedProjects, type ProjectState, type WindowRecord } from '../registry';

function project(folderPath: string, running = false): ProjectState {
    return { folderPath, name: folderPath.split('/').pop() ?? folderPath, running };
}

function record(partial: Partial<WindowRecord>): WindowRecord {
    return {
        windowId: 'w1',
        title: 'pitlane-docs',
        updatedAt: Date.now(),
        projects: [],
        ...partial,
    };
}

describe('ownedProjects', () => {
    it('yalnız kendi köklerini sahiplenir', () => {
        const peer = record({
            roots: ['/projects/pitlane-docs'],
            projects: [project('/projects/pitlane-docs'), project('/projects/garage-admin')],
        });

        expect(ownedProjects(peer).map((item) => item.folderPath)).toEqual(['/projects/pitlane-docs']);
    });

    it('kökü olmayan ama orada çalışan projeyi sahiplenir', () => {
        const peer = record({
            roots: ['/projects/pitlane-docs'],
            projects: [project('/projects/pitlane-docs'), project('/projects/apex-cms', true)],
        });

        expect(ownedProjects(peer).map((item) => item.folderPath)).toEqual([
            '/projects/pitlane-docs',
            '/projects/apex-cms',
        ]);
    });

    it('eski sürüm kaydında (roots yok) yalnız çalışanlar sayılır', () => {
        const peer = record({
            projects: [project('/projects/pitlane-docs'), project('/projects/garage-admin')],
        });

        expect(ownedProjects(peer)).toEqual([]);
    });

    it('eski sürüm kaydında çalışan proje yine görünür', () => {
        const peer = record({
            projects: [project('/projects/pitlane-docs', true), project('/projects/garage-admin')],
        });

        expect(ownedProjects(peer).map((item) => item.folderPath)).toEqual(['/projects/pitlane-docs']);
    });
});
