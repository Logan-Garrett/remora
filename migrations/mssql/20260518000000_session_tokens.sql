IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'session_tokens')
BEGIN
    CREATE TABLE session_tokens (
        id BIGINT IDENTITY(1,1) PRIMARY KEY,
        session_id UNIQUEIDENTIFIER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        token NVARCHAR(255) NOT NULL UNIQUE,
        label NVARCHAR(255) NOT NULL DEFAULT '',
        created_at DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET(),
        revoked_at DATETIMEOFFSET
    );
    CREATE INDEX idx_session_tokens_token ON session_tokens(token);
    CREATE INDEX idx_session_tokens_session ON session_tokens(session_id);
END
