## What this does

<!-- One or two sentences. -->

## Why

<!-- Link an issue or explain the motivation. -->

## Checklist

- [ ] `cargo fmt` and `cargo clippy -- -D warnings` pass
- [ ] `cd web && npx tsc --noEmit` passes
- [ ] Schema changes have migration files for all three backends (Postgres, SQLite, MSSQL)
- [ ] New MSSQL migrations are added to the list in `server/src/db/mssql.rs`
- [ ] No secrets in the diff
- [ ] Tests added or updated where relevant
