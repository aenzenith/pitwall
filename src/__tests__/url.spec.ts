import { describe, expect, it } from 'vitest';

import { extractLocalUrl, herdFallbackUrl, parseAppUrl } from '../resolve';

describe('extractLocalUrl', () => {
    it('vite çıktısındaki Local satırını yakalar', () => {
        const output = [
            '  VITE v7.1.5  ready in 412 ms',
            '',
            '  ➜  Local:   http://localhost:5173/',
            '  ➜  Network: use --host to expose',
        ].join('\n');

        expect(extractLocalUrl(output)).toBe('http://localhost:5173/');
    });

    it('ANSI renk kodlarını temizler', () => {
        const output = '[32m  ➜[39m  [1mLocal[22m:   [36mhttp://localhost:5174/[39m';

        expect(extractLocalUrl(output)).toBe('http://localhost:5174/');
    });

    it('Local etiketi yoksa çıplak yerel adresi bulur', () => {
        expect(extractLocalUrl('Server running at http://127.0.0.1:3000 now.')).toBe('http://127.0.0.1:3000');
    });

    it('yerel olmayan adresi almaz', () => {
        expect(extractLocalUrl('see https://vitejs.dev/config/ for help')).toBeUndefined();
        expect(extractLocalUrl('derleniyor…')).toBeUndefined();
    });
});

describe('parseAppUrl', () => {
    it('APP_URL değerini okur', () => {
        expect(parseAppUrl('APP_NAME=x\nAPP_URL=https://gridwall.test\nAPP_ENV=local')).toBe(
            'https://gridwall.test',
        );
    });

    it('satır sonu yorumunu değere katmaz', () => {
        const raw = 'APP_URL=https://apex-telemetry.test                # imzalı bağlantı tabanı';

        expect(parseAppUrl(raw)).toBe('https://apex-telemetry.test');
    });

    it('tırnaklı değeri açar', () => {
        expect(parseAppUrl('APP_URL="http://garage-admin.test"')).toBe('http://garage-admin.test');
    });

    it('boş ya da http olmayan değeri yok sayar', () => {
        expect(parseAppUrl('APP_URL=\nOTHER=1')).toBeUndefined();
        expect(parseAppUrl('APP_URL=gridwall.test')).toBeUndefined();
        expect(parseAppUrl('DB_HOST=127.0.0.1')).toBeUndefined();
    });
});

describe('herdFallbackUrl', () => {
    it('klasör adını kebab host yapar', () => {
        expect(herdFallbackUrl('apex-paddock')).toBe('https://apex-paddock.test');
        expect(herdFallbackUrl('Apex CMS')).toBe('https://apex-cms.test');
        expect(herdFallbackUrl('telemetry-v2')).toBe('https://telemetry-v2.test');
    });
});
