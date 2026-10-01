-- Ray model an anchor was taught with; the selector aims at it with the same model.
ALTER TABLE anchors ADD COLUMN ray_source TEXT NOT NULL DEFAULT 'finger_only'
  CHECK (ray_source IN ('eye_rooted','finger_only'));
