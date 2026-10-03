-- The schema of the typed data source fixture (tests/typed/shop.kbdata). The offline cache
-- `kubuno-data/.sqlx` was prepared against a SQLite database created from this file (see
-- tests/typed.rs, `regenerating the fixture cache`).
CREATE TABLE customers (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    email      TEXT,
    age        INTEGER,
    vip        BOOLEAN NOT NULL DEFAULT 0,
    birth_date DATE,
    balance    REAL
);

CREATE TABLE orders (
    id          INTEGER PRIMARY KEY,
    customer_id INTEGER NOT NULL REFERENCES customers (id),
    label       TEXT    NOT NULL,
    amount      REAL    NOT NULL
);

CREATE VIEW vip_customers AS SELECT id, name FROM customers WHERE vip = 1;
