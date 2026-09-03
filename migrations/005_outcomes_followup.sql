-- Post-apply outcomes + follow-up clock.
-- status stays the pipeline (applied). outcome is recruiter reality.
ALTER TABLE jobs ADD COLUMN applied_at TEXT;
ALTER TABLE jobs ADD COLUMN followed_up_at TEXT;
ALTER TABLE jobs ADD COLUMN outcome TEXT;

UPDATE jobs
SET applied_at = updated_at
WHERE status = 'applied' AND applied_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_jobs_applied_at ON jobs(applied_at);
CREATE INDEX IF NOT EXISTS idx_jobs_outcome ON jobs(outcome);
