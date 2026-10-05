-- Flick-only area for a camera; never written to Home Assistant.
ALTER TABLE cameras ADD COLUMN area_override TEXT;
