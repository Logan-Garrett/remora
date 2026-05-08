# Remora Desktop

Tauri v2 desktop application that wraps the Remora web client with native OS integration.

## Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [Node.js](https://nodejs.org/) (v18+)
- Tauri CLI: `cargo install tauri-cli`
- Platform-specific dependencies: see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

## Development

```bash
npm install
npm run dev
```

## Release Build

```bash
npm run build
```

Builds are output to `src-tauri/target/release/bundle/`.

## Features

- Native window with system tray icon
- Desktop notifications when Claude finishes responding
- Deep link support (`remora://join/...`) for one-click session joins

## Supported Platforms

- macOS (Universal)
- Windows (x64)
- Linux (AppImage, deb)
