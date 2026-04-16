-- MFA recovery codes and pending TOTP secret for two-step enrollment.

ALTER TABLE users
    ADD COLUMN mfa_totp_pending_secret TEXT,
    ADD COLUMN mfa_recovery_code_hashes TEXT[] NOT NULL DEFAULT '{}';
