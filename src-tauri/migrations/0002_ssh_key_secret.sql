-- M2：私钥正文落本机 SQLite；导入加密私钥时已解密，口令不落库
ALTER TABLE ssh_key ADD COLUMN private_key TEXT NOT NULL DEFAULT '';
