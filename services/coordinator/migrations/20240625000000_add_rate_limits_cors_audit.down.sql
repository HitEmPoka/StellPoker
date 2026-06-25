-- Rollback migration for issues #267, #261, #265

DROP TABLE IF EXISTS session_migrations CASCADE;
DROP TABLE IF EXISTS audit_logs CASCADE;
DROP TABLE IF EXISTS cors_configs CASCADE;
DROP TABLE IF EXISTS rate_limit_configs CASCADE;
