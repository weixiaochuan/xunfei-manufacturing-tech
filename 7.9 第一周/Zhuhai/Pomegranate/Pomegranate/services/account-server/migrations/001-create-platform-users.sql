CREATE TABLE platform_users (
    id UUID PRIMARY KEY,
    casdoor_owner TEXT NOT NULL,
    casdoor_name TEXT NOT NULL,
    account_number TEXT NOT NULL,
    display_name TEXT,
    email TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT platform_users_casdoor_identity_unique
        UNIQUE (casdoor_owner, casdoor_name),
    CONSTRAINT platform_users_account_number_unique
        UNIQUE (account_number),
    CONSTRAINT platform_users_casdoor_owner_not_blank
        CHECK (btrim(casdoor_owner) <> ''),
    CONSTRAINT platform_users_casdoor_name_not_blank
        CHECK (btrim(casdoor_name) <> ''),
    CONSTRAINT platform_users_account_number_not_blank
        CHECK (btrim(account_number) <> '')
);
