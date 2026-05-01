CREATE TABLE session_trusted (
    session_id UNIQUEIDENTIFIER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    participant_name NVARCHAR(255) NOT NULL,
    added_at DATETIMEOFFSET NOT NULL DEFAULT SYSDATETIMEOFFSET(),
    PRIMARY KEY (session_id, participant_name)
);
