IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'team_members')
BEGIN
    CREATE TABLE team_members (
        team_id UNIQUEIDENTIFIER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
        user_id UNIQUEIDENTIFIER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        role NVARCHAR(50) NOT NULL DEFAULT 'member',
        joined_at DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET(),
        PRIMARY KEY (team_id, user_id)
    );
END
