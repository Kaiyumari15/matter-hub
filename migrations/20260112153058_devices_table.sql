-- Add migration script here
CREATE TABLE devices (
    id INTEGER PRIMARY KEY,
    node_id INTEGER NOT NULL,
    endpoint_id INTEGER NOT NULL,
    device_type VARCHAR(50) NOT NULL,
    name VARCHAR(100) NOT NULL,
    capabilities TEXT NOT NULL DEFAULT '{}'
);