ALTER TABLE organizations
ADD COLUMN member_directory_is_private BOOLEAN NOT NULL DEFAULT FALSE;
