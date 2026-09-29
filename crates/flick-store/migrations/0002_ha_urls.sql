-- Home (LAN) URL and the Wi-Fi networks on which Flick should prefer it.
ALTER TABLE ha_instances ADD COLUMN internal_url TEXT;
ALTER TABLE ha_instances ADD COLUMN trusted_ssids TEXT;
