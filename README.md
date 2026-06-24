# v18b TLS frame input EOF fix

Fixes `aap-tls-handshake-probe` false `peer closed connection without sending TLS close_notify` after ClientHello.

The bug was using `Cursor<&[u8]>` for packetized TLS input. When consumed, `Cursor` returns `Ok(0)`, which rustls treats as real EOF. AAP TLS data arrives inside MESSAGE_ENCAPSULATED_SSL frames, so end-of-slice means “no more bytes in this AA frame yet”, not TCP EOF. The input reader now returns `WouldBlock` when the frame slice is consumed, matching aa-proxy-rs `SslMemBuf` behavior.
