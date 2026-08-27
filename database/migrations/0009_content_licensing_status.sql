-- Real Bible dataset production import milestone.
--
-- content_registry gains an explicit licensing_status column - what CIP
-- has independently concluded about a content item's right to be stored
-- and redistributed (cip_core_content::LicensingStatus), distinct from
-- the existing free-text license/distribution columns (which only record
-- what a source *said*). Defaults to 'unknown' so every pre-existing row
-- (the dev-seeded KJV fixture, any prior user-provided import) stays
-- honestly unclassified rather than silently upgraded to permissive.

ALTER TABLE content_registry
    ADD COLUMN licensing_status TEXT NOT NULL DEFAULT 'unknown'
        CHECK (licensing_status IN (
            'verified_public_domain', 'verified_redistributable',
            'licensed_for_cip', 'unknown', 'restricted'
        ));
