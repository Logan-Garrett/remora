# Remora JetBrains Plugin

IntelliJ Platform plugin for collaborative Claude Code sessions in JetBrains IDEs.

## Prerequisites

- JDK 17+
- IntelliJ IDEA 2024.1 or later (Community or Ultimate)

## Build

```bash
./gradlew buildPlugin
```

The plugin ZIP is output to `build/distributions/`.

## Run (Development)

```bash
./gradlew runIde
```

This launches a sandboxed IDE instance with the plugin installed.

## Features

- Tool window with chat panel (right sidebar)
- WebSocket connection to Remora server
- Slash commands (/run, /who, /diff, /add, etc.)
- Settings panel under Settings > Tools > Remora
- Status bar widget showing connection state
