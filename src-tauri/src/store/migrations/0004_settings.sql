-- App-wide settings the backend needs itself, such as closing to the tray. Display settings the
-- frontend alone uses stay in its local storage.
CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
