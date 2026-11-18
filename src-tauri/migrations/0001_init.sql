-- 会话分组（树形）
CREATE TABLE IF NOT EXISTS folder (
  id          TEXT PRIMARY KEY,
  parent_id   TEXT REFERENCES folder(id) ON DELETE CASCADE,
  name        TEXT NOT NULL,
  sort        INTEGER NOT NULL DEFAULT 0
);

-- 主机/会话
CREATE TABLE IF NOT EXISTS host (
  id                 TEXT PRIMARY KEY,
  folder_id          TEXT REFERENCES folder(id) ON DELETE SET NULL,
  alias              TEXT NOT NULL,
  host               TEXT NOT NULL,
  port               INTEGER NOT NULL DEFAULT 22,
  username           TEXT NOT NULL,
  auth_type          TEXT NOT NULL DEFAULT 'password', -- password | key | agent | keyboard-interactive
  key_id             TEXT,
  proxy_jump_id      TEXT REFERENCES host(id) ON DELETE SET NULL,
  term_type          TEXT NOT NULL DEFAULT 'xterm-256color',
  keepalive_secs     INTEGER NOT NULL DEFAULT 30,
  startup_snippet_id TEXT,
  env_json           TEXT,
  tags               TEXT,
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_host_folder ON host(folder_id);

-- 密钥元数据：私钥本体只存 OS Keyring，secret_ref 为其条目名
CREATE TABLE IF NOT EXISTS ssh_key (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  key_type    TEXT NOT NULL,
  bits        INTEGER,
  public_key  TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  secret_ref  TEXT NOT NULL,
  created_at  INTEGER NOT NULL
);

-- 已知主机密钥（TOFU）
CREATE TABLE IF NOT EXISTS known_host (
  host     TEXT NOT NULL,
  port     INTEGER NOT NULL,
  key_type TEXT NOT NULL,
  key_blob BLOB NOT NULL,
  added_at INTEGER NOT NULL,
  PRIMARY KEY (host, port, key_type)
);

-- 端口转发规则
CREATE TABLE IF NOT EXISTS forward_rule (
  id          TEXT PRIMARY KEY,
  host_id     TEXT NOT NULL REFERENCES host(id) ON DELETE CASCADE,
  type        TEXT NOT NULL, -- local | remote | dynamic
  bind_host   TEXT NOT NULL DEFAULT '127.0.0.1',
  bind_port   INTEGER NOT NULL,
  target_host TEXT,
  target_port INTEGER,
  auto_start  INTEGER NOT NULL DEFAULT 0,
  enabled     INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_forward_host ON forward_rule(host_id);

-- 命令片段
CREATE TABLE IF NOT EXISTS snippet (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  command    TEXT NOT NULL,
  tags       TEXT,
  updated_at INTEGER NOT NULL
);

-- 应用设置（键值）
CREATE TABLE IF NOT EXISTS app_setting (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
