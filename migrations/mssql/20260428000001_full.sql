-- Repos per session
CREATE TABLE session_repos (
    id          BIGINT IDENTITY(1,1) PRIMARY KEY,
    session_id  UNIQUEIDENTIFIER NOT NULL
        REFERENCES sessions(id) ON DELETE CASCADE,
    name        NVARCHAR(512)    NOT NULL,
    git_url     NVARCHAR(2048)   NOT NULL,
    added_at    DATETIMEOFFSET   NOT NULL DEFAULT SYSDATETIMEOFFSET(),
    CONSTRAINT uq_session_repos_session_name UNIQUE (session_id, name)
);

-- Track Claude runs
CREATE TABLE session_runs (
    id              BIGINT IDENTITY(1,1) PRIMARY KEY,
    session_id      UNIQUEIDENTIFIER NOT NULL
        REFERENCES sessions(id) ON DELETE CASCADE,
    started_at      DATETIMEOFFSET   NOT NULL DEFAULT SYSDATETIMEOFFSET(),
    finished_at     DATETIMEOFFSET,
    status          NVARCHAR(64)     NOT NULL DEFAULT 'running',
    owner_instance  NVARCHAR(256),
    heartbeat       DATETIMEOFFSET   NOT NULL DEFAULT SYSDATETIMEOFFSET(),
    context_mode    NVARCHAR(64)     NOT NULL DEFAULT 'since_last'
);

-- Fetch allowlists
CREATE TABLE global_allowlist (
    domain NVARCHAR(512) PRIMARY KEY,
    kind   NVARCHAR(64)  NOT NULL DEFAULT 'allow'
);

CREATE TABLE session_allowlist (
    session_id  UNIQUEIDENTIFIER NOT NULL
        REFERENCES sessions(id) ON DELETE CASCADE,
    domain      NVARCHAR(512)    NOT NULL,
    approved_at DATETIMEOFFSET   NOT NULL DEFAULT SYSDATETIMEOFFSET(),
    PRIMARY KEY (session_id, domain)
);

-- Quotas
ALTER TABLE sessions ADD daily_token_cap   BIGINT DEFAULT 999999999;
ALTER TABLE sessions ADD tokens_used_today BIGINT DEFAULT 0;
ALTER TABLE sessions ADD tokens_reset_date DATE   DEFAULT CAST(GETDATE() AS DATE);
ALTER TABLE sessions ADD idle_since        DATETIMEOFFSET;

-- Pending fetch approvals
CREATE TABLE pending_approvals (
    id            BIGINT IDENTITY(1,1) PRIMARY KEY,
    session_id    UNIQUEIDENTIFIER NOT NULL
        REFERENCES sessions(id) ON DELETE CASCADE,
    domain        NVARCHAR(512)    NOT NULL,
    url           NVARCHAR(2048)   NOT NULL,
    requested_by  NVARCHAR(512)    NOT NULL,
    requested_at  DATETIMEOFFSET   NOT NULL DEFAULT SYSDATETIMEOFFSET(),
    resolved      BIT              NOT NULL DEFAULT 0,
    approved      BIT
);
