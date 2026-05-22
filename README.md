# crest

Modular status bar for Windows, Direct2D-accelerated.

- Top-of-screen bar with configurable modules: workspaces, focused window, clock, CPU, memory, battery, network, volume, system tray, custom (shell-exec → JSON)
- Per-module click / scroll handlers
- CSS-ish theming via KDL
- Subscribes to wiri IPC event stream (and other WM IPC schemas in future)
- Multi-monitor (one bar per monitor, or primary only)
- Hot config reload

## Status

Early scaffolding. See `docs/PLAN.md`.

## Pairs with

- [`wiri`](https://github.com/jolionlands/wiri) — Windows tiling window manager. Crest reads wiri's IPC for live workspace state.
- [`aurora`](https://github.com/jolionlands/aurora) — desktop background swapper. Crest can show "current wallpaper" via a custom module.
