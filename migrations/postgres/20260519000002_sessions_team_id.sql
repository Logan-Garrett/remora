ALTER TABLE sessions ADD COLUMN team_id UUID REFERENCES teams(id);
CREATE INDEX idx_sessions_team ON sessions(team_id);
