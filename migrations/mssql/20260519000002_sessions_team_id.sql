IF NOT EXISTS (SELECT * FROM sys.columns WHERE object_id = OBJECT_ID('sessions') AND name = 'team_id')
BEGIN
    ALTER TABLE sessions ADD team_id UNIQUEIDENTIFIER NULL REFERENCES teams(id);
END

IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = 'idx_sessions_team')
BEGIN
    CREATE INDEX idx_sessions_team ON sessions(team_id);
END
