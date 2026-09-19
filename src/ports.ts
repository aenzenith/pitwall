import { execFile } from 'child_process';
import * as net from 'net';

/**
 * Port dinleniyor mu. Bağlanma denemesi yapar; 400 ms'de cevap yoksa boş sayar.
 */
export function isPortInUse(port: number, host = '127.0.0.1'): Promise<boolean> {
    return new Promise((resolve) => {
        const socket = new net.Socket();
        let settled = false;

        const finish = (inUse: boolean): void => {
            if (settled) {
                return;
            }

            settled = true;
            socket.destroy();
            resolve(inUse);
        };

        socket.setTimeout(400);
        socket.once('connect', () => finish(true));
        socket.once('timeout', () => finish(false));
        socket.once('error', () => finish(false));
        socket.connect(port, host);
    });
}

/**
 * `from` portundan başlayarak boş port arar.
 *
 * @param tries Kaç port denenecek.
 */
export async function findFreePort(from: number, tries = 20): Promise<number | undefined> {
    for (let port = from; port < from + tries && port < 65536; port += 1) {
        if (!(await isPortInUse(port))) {
            return port;
        }
    }

    return undefined;
}

/**
 * Portu dinleyen süreci tarif eder: `node · pid 48213`.
 * Yalnız macOS/Linux (lsof). Bulunamazsa undefined.
 */
export function describePortHolder(port: number): Promise<string | undefined> {
    if (!Number.isInteger(port) || port <= 0 || port > 65535) {
        return Promise.resolve(undefined);
    }

    if (process.platform === 'win32') {
        return Promise.resolve(undefined);
    }

    return new Promise((resolve) => {
        execFile(
            'lsof',
            ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN', '-Fcp'],
            { timeout: 2000 },
            (error, stdout) => {
                if (error || !stdout) {
                    resolve(undefined);

                    return;
                }

                resolve(parseLsof(stdout));
            },
        );
    });
}

/**
 * `lsof -F` çıktısından ilk sürecin adını ve pid'ini çıkarır.
 * Satırlar tek harf alan öneki taşır: `p<pid>`, `c<command>`.
 */
export function parseLsof(output: string): string | undefined {
    let pid: string | undefined;
    let command: string | undefined;

    for (const line of output.split(/\r?\n/)) {
        if (line.startsWith('p')) {
            pid = line.slice(1).trim();
        } else if (line.startsWith('c')) {
            command = line.slice(1).trim();
        }

        if (pid && command) {
            return `${command} · pid ${pid}`;
        }
    }

    return pid ? `pid ${pid}` : undefined;
}

/**
 * Yerel URL'den port çıkarır. Port yoksa şemaya göre varsayılan döner.
 */
export function portFromUrl(url: string): number | undefined {
    try {
        const parsed = new URL(url);

        if (parsed.port) {
            return Number(parsed.port);
        }

        return parsed.protocol === 'https:' ? 443 : 80;
    } catch {
        return undefined;
    }
}
