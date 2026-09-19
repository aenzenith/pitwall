import { describe, expect, it } from 'vitest';

import { parseVitePort } from '../resolve';

describe('parseVitePort', () => {
    it('server bloğundaki portu bulur', () => {
        const source = `
            export default defineConfig({
                plugins: [vue()],
                server: {
                    host: 'localhost',
                    port: 5180,
                },
            });
        `;

        expect(parseVitePort(source)).toBe(5180);
    });

    it('server bloğu yoksa undefined', () => {
        expect(parseVitePort('export default defineConfig({ plugins: [vue()] });')).toBeUndefined();
    });

    it('yorum satırındaki portu almaz', () => {
        const source = `
            // server: { port: 4444 }
            /* eski ayar: port: 3333 */
            export default defineConfig({ plugins: [] });
        `;

        expect(parseVitePort(source)).toBeUndefined();
    });

    it('server dışındaki portu almaz', () => {
        const source = `
            export default defineConfig({
                preview: { port: 4173 },
                plugins: [],
            });
        `;

        expect(parseVitePort(source)).toBeUndefined();
    });

    it('hmr portunu server portu sanmaz', () => {
        const source = `
            export default defineConfig({
                server: {
                    port: 5190,
                    hmr: { port: 5191 },
                },
            });
        `;

        expect(parseVitePort(source)).toBe(5190);
    });
});
