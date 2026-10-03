-- The toy entity of the engine tests: the synced columns plus the engine columns.
CREATE TABLE notes (
    id       TEXT PRIMARY KEY,
    title    TEXT,
    body     TEXT,
    _etag    TEXT,
    _seq     INTEGER,
    _pending INTEGER NOT NULL DEFAULT 0
);
