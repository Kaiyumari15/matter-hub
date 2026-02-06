-- Add migration script here
CREATE TABLE devices (
    id INTEGER PRIMARY KEY,
    node_id INTEGER NOT NULL,
    endpoint_id INTEGER NOT NULL,
    name VARCHAR(100) NOT NULL,
    capabilities TEXT NOT NULL DEFAULT '{}'
);