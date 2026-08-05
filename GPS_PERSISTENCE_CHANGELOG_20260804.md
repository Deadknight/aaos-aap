# ARDA DHU GPS persistence — 2026-08-04

Based on `aaosaap04.zip`.

Changes:

- `src/config.rs` accepts and resolves:
  - `sensor.period_ms`
  - `sensor.forward_os`
  - `sensor.location_provider`
  - `sensor.location_min_ms`
- `sensor.forward_os` resolves to the location-only CLI flags, so it does not
  implicitly enable VHAL forwarding.
- Root and packaged `arda-aap.toml` profiles now persist the live-tested
  location-only settings:
  - sensor channel enabled
  - request only `SENSOR_LOCATION` (type 1)
  - 200 ms request/minimum forwarding interval
  - forward to `arda_gps`
  - VHAL forwarding disabled
  - JSONL sensor dump disabled

No `repo sync`, commit, or push is performed by the deployment package.
