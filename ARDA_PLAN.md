# ARDA v18b

- Fixed TLS handshake probe memory input semantics.
- Replaced Cursor-based TLS read with WouldBlock-on-empty frame reader.
- Expected: after ClientHello, rustls generates ServerHello/Certificate/etc and we send MESSAGE_ENCAPSULATED_SSL back instead of EOF error.

### v19
- Added `aap-tls-plain-probe`.
- Keeps the DHU connection open after TLS handshake.
- Decrypts encrypted post-TLS AA frame payloads and logs plaintext message IDs.
- Next expected milestone: observe `MESSAGE_SERVICE_DISCOVERY_REQUEST` and reply with a minimal service discovery response.

## v20

- Added AAP proto codegen scaffold from aa-proxy-rs `src/protos`.
- Added `aap-proto-smoke`.
- Control-message hints now decode protobuf-backed messages via generated types, starting with `MESSAGE_AUTH_COMPLETE`.
- Next: build and send ServiceDiscovery using generated proto structs instead of manual payloads.
