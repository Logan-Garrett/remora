CREATE TABLE session_trusted (
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    participant_name TEXT NOT NULL,
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (session_id, participant_name)
);
