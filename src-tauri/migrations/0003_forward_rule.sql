-- forward_rule 在 0001 已建表，这里只补时间戳字段
ALTER TABLE forward_rule ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE forward_rule ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
