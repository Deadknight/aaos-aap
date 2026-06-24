# ARDA v18b

- Fixed TLS handshake probe memory input semantics.
- Replaced Cursor-based TLS read with WouldBlock-on-empty frame reader.
- Expected: after ClientHello, rustls generates ServerHello/Certificate/etc and we send MESSAGE_ENCAPSULATED_SSL back instead of EOF error.
