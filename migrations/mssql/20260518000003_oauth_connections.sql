IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'oauth_connections')
BEGIN
    CREATE TABLE oauth_connections (
        id BIGINT IDENTITY(1,1) PRIMARY KEY,
        user_id UNIQUEIDENTIFIER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        provider NVARCHAR(100) NOT NULL,
        provider_user_id NVARCHAR(512) NOT NULL,
        created_at DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET(),
        UNIQUE(provider, provider_user_id)
    );
END
