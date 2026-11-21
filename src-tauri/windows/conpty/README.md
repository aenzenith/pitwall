# ConPTY, bundled for Windows

Pitwall's terminals on Windows run on this ConPTY instead of the one Windows has built in.
portable-pty loads `conpty.dll` from beside the executable when it is there (`load_conpty` in
its `win/psuedocon.rs`), and that `conpty.dll` starts the `OpenConsole.exe` beside it as each
terminal's console. `tauri.windows.conf.json` puts both next to `pitwall.exe` in the installers
(and in `target/` for a dev build); the core tells the window that they are there
(`conpty_bundled`), and the window's xterm is set for them (`src/lib/terminals.ts`).

Windows 10's own ConPTY is from before 2020: it passes no mouse to the program in a terminal (so
Claude Code's fullscreen view couldn't be scrolled with the wheel) and keeps SI and SO as
characters (a letter stuck at Claude's prompt). This one is the console host Windows Terminal
ships.

- Source: the NuGet package `Microsoft.Windows.Console.ConPTY` 1.25.261109001 (Microsoft, MIT),
  its SHA-512 the one NuGet's catalog gives;
  `runtimes/win-x64/native/conpty.dll` and `build/native/runtimes/x64/OpenConsole.exe`, both
  signed by Microsoft Corporation (Microsoft Code Signing PCA 2024).
- SHA-256:
  - `conpty.dll` e971856f89748dc3b458125e9afdd79b5e9574a7c937676116d3cbbd3b71f492
  - `OpenConsole.exe` b17e48b07ef153b420f0a6e178bacd31fb4c3390e21ae746586213fc0719a775
- Licence: `LICENSE` (microsoft/terminal's MIT licence).

x64 only, as the Windows installers are. To update, take both files from a newer release of the
same package and write its version and their hashes here.
