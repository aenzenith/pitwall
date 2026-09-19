import { describe, expect, it } from 'vitest';

import { detectErrorLine } from '../resolve';

const ESC = String.fromCharCode(27);

describe('detectErrorLine', () => {
    it('çözülemeyen import satırını yakalar', () => {
        const output = [
            '  VITE v7.3.6  ready in 2705 ms',
            '  Failed to resolve import "@/pages/Yok.vue" from "resources/js/app.ts"',
        ].join('\n');

        expect(detectErrorLine(output)).toContain('Failed to resolve import');
    });

    it('node hata türlerini yakalar', () => {
        expect(detectErrorLine('SyntaxError: Unexpected token }')).toBe('SyntaxError: Unexpected token }');
        expect(detectErrorLine('Error: Cannot find module "vite"')).toContain('Cannot find module');
    });

    it('npm hata satırını yakalar', () => {
        expect(detectErrorLine('npm ERR! code ELIFECYCLE')).toContain('npm ERR!');
    });

    it('ANSI kodlarını temizler', () => {
        const output = `${ESC}[31mnpm ERR!${ESC}[39m missing script: dev`;

        expect(detectErrorLine(output)).toBe('npm ERR! missing script: dev');
    });

    it('uzun satırı kısaltır', () => {
        const long = `SyntaxError: ${'x'.repeat(300)}`;
        const result = detectErrorLine(long, 40);

        expect(result).toHaveLength(40);
        expect(result?.endsWith('…')).toBe(true);
    });

    it('normal çıktıda hata görmez', () => {
        expect(detectErrorLine('  ➜  Local:   http://localhost:5173/')).toBeUndefined();
        expect(detectErrorLine('LARAVEL v13.26.0  plugin v2.1.0')).toBeUndefined();
        // "error" kelimesi geçse de vite etiketi yoksa sayılmaz.
        expect(detectErrorLine('watching for file changes, no error handler set')).toBeUndefined();
    });
});
