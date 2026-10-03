-- Flick-only area for a taught device; never written to Home Assistant.
ALTER TABLE anchors ADD COLUMN area_override TEXT;
