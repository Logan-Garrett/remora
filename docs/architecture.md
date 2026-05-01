# Remora — Architecture

A deep-dive reference for contributors and operators. See [README.md](../README.md) for a quick overview.

---

## System Components

```mermaid
flowchart TB
    subgraph clients ["Clients"]
        WEB["🌐 Web Browser\nTypeScript / Vite"]
        NV["📝 Neovim Plugin\nLua"]
    end

    subgraph bridge ["Bridge Binary (Rust)"]
        BR["stdio ↔ WebSocket\nbridge/src/main.rs"]
    end

    NV -- "JSON over stdio" --> BR
    BR -- "WebSocket" --> SRV
    WEB -- "WebSocket + REST" --> SRV

    subgraph server ["Server — Rust / axum"]
        SRV["HTTP + WebSocket\nlib.rs · ws.rs"]
        CMD["Command Dispatch\ncommands.rs"]
        STATE["In-Memory State\nstate.rs"]
        QUOTA["Quota + Idle Cleanup\nquota.rs"]
        SANDBOX["Docker Sandbox\nsandbox.rs"]
        SRV --> CMD
        CMD --> STATE
        CMD --> SANDBOX
        STATE --> QUOTA
    end

    subgraph db ["Database (one of three)"]
        PG["🐘 PostgreSQL\nLISTEN / NOTIFY"]
        SQ["🪶 SQLite\nin-process broadcast"]
        MS["🏢 MSSQL\nin-process broadcast"]
    end

    CMD -- "persist events" --> PG & SQ & MS
    PG & SQ & MS -- "notify new event" --> STATE

    subgraph claude ["Claude"]
        CLI["Claude CLI\n--output-format stream-json"]
    end

    CMD -- "spawn process" --> CLI
    CLI -- "tool calls · responses" --> CMD

    subgraph sandbox_box ["Docker Sandbox (optional, per-session)"]
        DC["Isolated container\nREMORA_USE_SANDBOX=true"]
    end

    SANDBOX --> DC
```

---

## Database Schema

```mermaid
erDiagram
    sessions {
        uuid        id              PK
        text        description
        timestamptz created_at
        timestamptz updated_at
        bigint      daily_token_cap
        bigint      tokens_used_today
        date        tokens_reset_date
        timestamptz idle_since
    }

    events {
        bigint      id         PK
        uuid        session_id FK
        timestamptz timestamp
        text        author
        text        kind
        jsonb       payload
    }

    session_repos {
        bigint      id         PK
        uuid        session_id FK
        text        name
        text        git_url
        timestamptz added_at
    }

    session_runs {
        bigint      id             PK
        uuid        session_id     FK
        timestamptz started_at
        timestamptz finished_at
        text        status
        text        owner_instance
        timestamptz heartbeat
        text        context_mode
    }

    global_allowlist {
        text domain PK
        text kind
    }

    session_allowlist {
        uuid        session_id  FK
        text        domain
        timestamptz approved_at
    }

    pending_approvals {
        bigint      id           PK
        uuid        session_id   FK
        text        domain
        text        url
        text        requested_by
        timestamptz requested_at
        boolean     resolved
        boolean     approved
    }

    sessions ||--o{ events            : "append-only log"
    sessions ||--o{ session_repos     : "cloned repos"
    sessions ||--o{ session_runs      : "claude runs"
    sessions ||--o{ session_allowlist : "approved domains"
    sessions ||--o{ pending_approvals : "pending fetch requests"
```

### Event `kind` values

| kind | Emitted by | Description |
|---|---|---|
| `chat` | any client | Plain chat message |
| `system` | server | Join/leave/info/help/error messages |
| `claude_response` | Claude CLI | A full response turn from Claude |
| `tool_call` | Claude CLI | A tool invocation (file edit, bash, etc.) |
| `tool_result` | Claude CLI | Output from a tool call |
| `file` | `/add` command | Inlined file content |
| `diff` | `/diff` command | Git diff output |
| `fetch` | `/fetch` command | Fetched URL content |
| `clear_marker` | `/clear` command | Context reset point |
| `kick` | `/kick` command | Participant removal notice |

---

## `/run` Sequence

How a Claude run flows from the moment a user types `/run` to every participant seeing the response.

```mermaid
sequenceDiagram
    participant A  as Client A (runner)
    participant B  as Client B (observer)
    participant WS as ws.rs
    participant CM as commands.rs
    participant DB as Database
    participant EL as event_listener
    participant CL as Claude CLI

    A  ->> WS: ClientMsg::Run { author }
    WS ->> CM: dispatch(Run)
    CM ->> DB: is_run_in_flight(session_id)?
    DB -->> CM: false
    CM ->> DB: insert_run(status = "running")
    CM ->> DB: insert_event(kind = "system", "started a Claude run")
    DB -->> EL: notify(event_id)
    EL ->> WS: dispatch(event)
    WS -->> A: ServerMsg::Event
    WS -->> B: ServerMsg::Event

    CM ->> CL: spawn claude (stream-json, up to 5 turns)

    loop agentic turn (repeats up to 5×)
        CL -->> CM: tool_call JSON
        CM ->> DB: insert_event(kind = "tool_call")
        DB -->> EL: notify
        EL ->> WS: dispatch
        WS -->> A: tool_call event
        WS -->> B: tool_call event

        CL -->> CM: tool_result JSON
        CM ->> DB: insert_event(kind = "tool_result")
        DB -->> EL: notify
        EL ->> WS: dispatch
        WS -->> A: tool_result event
        WS -->> B: tool_result event

        CL -->> CM: claude_response JSON
        CM ->> DB: insert_event(kind = "claude_response")
        DB -->> EL: notify
        EL ->> WS: dispatch
        WS -->> A: claude_response event
        WS -->> B: claude_response event
    end

    CM ->> DB: update_run(status = "completed")
    CM ->> DB: insert_event(kind = "system", "run completed")
    DB -->> EL: notify
    EL ->> WS: dispatch
    WS -->> A: system event
    WS -->> B: system event
```

---

## WebSocket Connection Lifecycle

```mermaid
stateDiagram-v2
    [*]          --> Connecting   : user joins session
    Connecting   --> Connected    : handshake OK · backfill replayed
    Connecting   --> [*]          : 401 Unauthorized
    Connected    --> Running      : /run dispatched
    Running      --> Connected    : run completed / failed / timeout
    Connected    --> Disconnected : network drop / server restart
    Disconnected --> Connecting   : auto-reconnect (bridge: up to 3×)
    Disconnected --> [*]          : max retries exceeded
    Connected    --> [*]          : /leave or kicked
```

Notes:
- The server sends a **30-second WebSocket ping** to prevent Cloudflare and other proxies from closing idle connections.
- On reconnect, the server replays up to `REMORA_BACKFILL_LIMIT` (default 500) recent events so the client catches up.
- The bridge binary handles reconnect logic. The web client does not auto-reconnect — the user re-opens or refreshes.

---

## Web Client Navigation

```mermaid
stateDiagram-v2
    [*]      --> Login    : page load · no saved config
    [*]      --> Sessions : page load · config in sessionStorage
    Login    --> Sessions : health check OK · credentials accepted
    Sessions --> Chat     : join or create session
    Chat     --> Sessions : leave session
    Sessions --> Login    : disconnect
    Chat     --> Login    : disconnect (from chat header)
```

The web client stores `{ url, token, name }` in `sessionStorage` after a successful login. Refreshing the page skips the login screen. Clicking **Disconnect** clears it and returns to login.

---

## Multi-Instance Deployment

```mermaid
flowchart LR
    subgraph lb ["Load Balancer / Tunnel"]
        LB["nginx · Cloudflare · etc."]
    end

    subgraph instances ["Server Instances"]
        S1["remora-server :7200"]
        S2["remora-server :7201"]
    end

    subgraph state ["Shared State"]
        PG["PostgreSQL\nLISTEN / NOTIFY"]
        FS["Shared Filesystem\nor object store"]
    end

    LB --> S1 & S2
    S1 & S2 --> PG
    S1 & S2 --> FS

    style lb fill:#2f3350,stroke:#7aa2f7
    style state fill:#2f3350,stroke:#9ece6a
```

**Works today with Postgres.** LISTEN/NOTIFY crosses process boundaries — when instance S1 writes an event, S2 gets the notification and fans it out to its own subscribers.

**Does not work with SQLite or MSSQL** — their notification path is in-process only.

**Remaining gap:** the `participants` map (who is online) is still per-instance. `/who` only shows users connected to the same instance. Moving presence to the DB is on the roadmap.

---

## Notification Backends

| Backend | Notification mechanism | Multi-instance safe |
|---|---|---|
| PostgreSQL | `pg_notify` + `LISTEN` | ✅ Yes |
| SQLite | `tokio::sync::broadcast` | ❌ Single instance only |
| MSSQL | `tokio::sync::broadcast` | ❌ Single instance only |
