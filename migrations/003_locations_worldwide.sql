-- Empty locations = no country filter (worldwide). Clear the old Norway/EU default.
UPDATE settings
SET locations = '',
    updated_at = datetime('now')
WHERE id = 1
  AND (
    locations = 'norway,oslo,remote,europe'
    OR locations = 'norway, oslo, remote, europe'
  );
