# Pitwall

*Your projects' dev servers and Claude sessions, one click away in the menu bar.*

Works together with the [Pitwall VS Code extension](https://github.com/aenzenith/pitwall-vscode).

## Platforms

- **macOS 13+**: menu bar icon with a popover.
- **Windows 10/11**: tray icon. Needs the WebView2 runtime, which Windows 11 and up-to-date
  Windows 10 already have.
- **Linux**: tray icon through AppIndicator; needs webkit2gtk-4.1. On GNOME, install the
  AppIndicator extension. Clicking the icon opens a menu rather than the popover.

## Install

Download the latest build from [Releases](../../releases). Builds aren't signed yet, so the
system asks once:

- **macOS** (`.dmg`): drag Pitwall to Applications, then right-click it › Open › Open.
- **Windows** (`-setup.exe` or `.msi`): on the SmartScreen prompt, More info › Run anyway.
- **Linux**: `chmod +x Pitwall_*.AppImage` and run it, or `sudo apt install ./Pitwall_*.deb`.

## Development

Needs Node 22 and stable Rust, plus per platform:

- **macOS**: Xcode Command Line Tools (`xcode-select --install`).
- **Windows**: Microsoft C++ Build Tools (Desktop development with C++) and WebView2.
- **Linux**: the packages in [`tools/linux-check/packages.txt`](tools/linux-check/packages.txt):
  `sudo apt install $(sed 's/#.*//' tools/linux-check/packages.txt)`.

```sh
npm ci
npm run tauri dev
npx vitest run && cargo test --manifest-path src-tauri/Cargo.toml
```

No Linux machine? `tools/linux-check/run.sh` runs the same checks as CI in Docker.

## Licence

MIT
