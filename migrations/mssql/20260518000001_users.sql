IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'users')
BEGIN
    CREATE TABLE users (
        id UNIQUEIDENTIFIER PRIMARY KEY DEFAULT NEWID(),
        email NVARCHAR(512) NOT NULL UNIQUE,
        display_name NVARCHAR(255) NOT NULL,
        password_hash NVARCHAR(1024),
        role NVARCHAR(50) NOT NULL DEFAULT 'member',
        created_at DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET(),
        updated_at DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET()
    );
    CREATE INDEX idx_users_email ON users(email);
END
