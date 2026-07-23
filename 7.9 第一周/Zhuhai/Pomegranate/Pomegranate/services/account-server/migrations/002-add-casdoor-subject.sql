CREATE SEQUENCE platform_account_number_seq
    AS INTEGER
    MINVALUE 1
    MAXVALUE 999999
    START WITH 1
    NO CYCLE;

ALTER TABLE platform_users
    ADD COLUMN casdoor_subject TEXT;

ALTER TABLE platform_users
    ALTER COLUMN casdoor_subject SET NOT NULL,
    ADD CONSTRAINT platform_users_casdoor_subject_unique
        UNIQUE (casdoor_subject),
    ADD CONSTRAINT platform_users_casdoor_subject_not_blank
        CHECK (btrim(casdoor_subject) <> '');
