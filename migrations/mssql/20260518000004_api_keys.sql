IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'api_keys')
BEGIN
    CREATE TABLE api_keys (
        id UNIQUEIDENTIFIER PRIMARY KEY DEFAULT NEWID(),
        user_id UNIQUEIDENTIFIER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        key_hash NVARCHAR(512) NOT NULL UNIQUE,
        label NVARCHAR(255) NOT NULL DEFAULT '',
        created_at DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET(),
        last_used_at DATETIMEOFFSET,
        revoked_at DATETIMEOFFSET
    );
    CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);
END
