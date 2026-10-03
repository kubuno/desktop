-- Deliberately broken (test of the read-only fallback).
ALTER TABLE no_such_table ADD COLUMN x TEXT;
