-- Sessions table
CREATE TABLE sessions (
    id              UNIQUEIDENTIFIER PRIMARY KEY DEFAULT NEWID(),
    description     NVARCHAR(MAX)    NOT NULL DEFAULT '',
    created_at      DATETIMEOFFSET   NOT NULL DEFAULT SYSDATETIMEOFFSET(),
    updated_at      DATETIMEOFFSET   NOT NULL DEFAULT SYSDATETIMEOFFSET()
);

-- Events table
CREATE TABLE events (
    id          BIGINT IDENTITY(1,1) PRIMARY KEY,
    session_id  UNIQUEIDENTIFIER NOT NULL
        REFERENCES sessions(id) ON DELETE CASCADE,
    [timestamp] DATETIMEOFFSET   NOT NULL DEFAULT SYSDATETIMEOFFSET(),
    author      NVARCHAR(512),
    kind        NVARCHAR(256)    NOT NULL,
    payload     NVARCHAR(MAX)    NOT NULL
);

CREATE INDEX idx_events_session_id ON events (session_id, id);
