//! Minimal rustls TLS-in-AAP helper.
//!
//! AAP carries TLS records inside MESSAGE_ENCAPSULATED_SSL control frames. This
//! module only owns the TLS state machine; src/aap/mod.rs keeps the AA framing.
//! The current role is phone-like: TCP server, TLS server, md_cert/md_key.

use rustls::crypto::{aws_lc_rs, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::sign::CertifiedKey;
use rustls::version::TLS12;
use rustls::{DigitallySignedStruct, DistinguishedName, ServerConfig, ServerConnection, SignatureScheme};
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::sync::Arc;

pub struct TlsServerStep {
    pub outgoing_tls: Vec<u8>,
    pub plaintext: Vec<u8>,
    pub still_handshaking: bool,
    pub state: String,
    pub cipher_suite: String,
}

pub struct AapTlsServer {
    conn: ServerConnection,
}

impl AapTlsServer {
    pub fn new_phone_like(keys_path: &Path) -> io::Result<Self> {
        let resolved_keys_path = crate::aap_key_update::resolve_keys_dir(keys_path);
        let cert_path = resolved_keys_path.join("md_cert.pem");
        let key_path = resolved_keys_path.join("md_key.pem");
        if resolved_keys_path != keys_path {
            println!("ARDA_AAP_KEY_UPDATE_KEYS=ACTIVE path={}", resolved_keys_path.display());
        }
        let (certs, key) = load_cert_and_key(&cert_path, &key_path)?;
        let cert0 = certs
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "md_cert.pem contains no certificate"))?;

        let provider = default_provider();
        let resolver = AaServerCertResolver::new(cert0, key)?;
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&TLS12])
            .map_err(io_other)?
            .with_client_cert_verifier(AaClientCertVerifier::new())
            .with_cert_resolver(resolver);

        // Match aa-proxy-rs behavior: avoid resumption/tickets while probing.
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        config.send_tls13_tickets = 0;

        let conn = ServerConnection::new(Arc::new(config)).map_err(io_other)?;
        Ok(Self { conn })
    }

    pub fn feed_tls(&mut self, tls_bytes: &[u8]) -> io::Result<TlsServerStep> {
        // Important: do NOT use std::io::Cursor directly here. When Cursor is
        // exhausted it returns Ok(0), and rustls interprets that as a real TCP
        // EOF. AAP encapsulation gives us packetized TLS bytes; reaching the
        // end of this slice only means "no more bytes in this AA frame yet",
        // so the reader must return WouldBlock when consumed. This mirrors the
        // aa-proxy-rs SslMemBuf behavior and avoids the false
        // "peer closed connection without close_notify" error after
        // ClientHello.
        let mut input = TlsFrameInput::new(tls_bytes);
        loop {
            match self.conn.read_tls(&mut input) {
                Ok(0) => break,
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(err),
            }
        }

        self.conn.process_new_packets().map_err(io_other)?;

        let mut outgoing_tls = Vec::new();
        while self.conn.wants_write() {
            self.conn.write_tls(&mut outgoing_tls).map_err(io_other)?;
        }

        let mut plaintext = Vec::new();
        match self.conn.reader().read_to_end(&mut plaintext) {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
            Err(err) => return Err(err),
        }

        Ok(TlsServerStep {
            outgoing_tls,
            plaintext,
            still_handshaking: self.conn.is_handshaking(),
            state: self.state_string(),
            cipher_suite: self.cipher_suite().to_string(),
        })
    }

    pub fn is_handshaking(&self) -> bool {
        self.conn.is_handshaking()
    }

    pub fn state_string(&self) -> String {
        if self.conn.is_handshaking() {
            "TLS handshake in progress".to_string()
        } else {
            let version = self
                .conn
                .protocol_version()
                .map(|v| format!("{v:?}"))
                .unwrap_or_else(|| "unknown version".to_string());
            let resumed = matches!(self.conn.handshake_kind(), Some(rustls::HandshakeKind::Resumed));
            format!(
                "SSL negotiation finished successfully ({version}{})",
                if resumed { ", resumed" } else { "" }
            )
        }
    }

    pub fn cipher_suite(&self) -> &'static str {
        self.conn
            .negotiated_cipher_suite()
            .map(|s| s.suite().as_str().unwrap_or("unknown"))
            .unwrap_or("unknown")
    }

    /// Encrypt one plaintext AA application/control payload into TLS records.
    /// The returned bytes are the payload of an AA frame with ENCRYPTED set.
    pub fn encrypt_plaintext(&mut self, plaintext: &[u8]) -> io::Result<Vec<u8>> {
        self.conn.writer().write_all(plaintext)?;
        let mut outgoing_tls = Vec::new();
        while self.conn.wants_write() {
            self.conn.write_tls(&mut outgoing_tls).map_err(io_other)?;
        }
        Ok(outgoing_tls)
    }
}

fn default_provider() -> Arc<CryptoProvider> {
    Arc::new(aws_lc_rs::default_provider())
}


struct TlsFrameInput<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> TlsFrameInput<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl Read for TlsFrameInput<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = &self.data[self.pos..];
        if remaining.is_empty() {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        let n = buf.len().min(remaining.len());
        buf[..n].copy_from_slice(&remaining[..n]);
        self.pos += n;
        Ok(n)
    }
}

fn load_cert_and_key(
    cert_path: &Path,
    key_path: &Path,
) -> io::Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_file = File::open(cert_path).map_err(|e| {
        io::Error::new(e.kind(), format!("cannot open cert {}: {e}", cert_path.display()))
    })?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(io_other)?;

    let key = load_private_key(key_path)?;
    Ok((certs, key))
}

fn load_private_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    let key_file = File::open(path).map_err(|e| {
        io::Error::new(e.kind(), format!("cannot open key {}: {e}", path.display()))
    })?;
    let mut reader = BufReader::new(key_file);
    if let Some(key) = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .next()
        .transpose()
        .map_err(io_other)?
    {
        return Ok(PrivateKeyDer::Pkcs8(key));
    }

    let key_file = File::open(path).map_err(|e| {
        io::Error::new(e.kind(), format!("cannot reopen key {}: {e}", path.display()))
    })?;
    let mut reader = BufReader::new(key_file);
    if let Some(key) = rustls_pemfile::rsa_private_keys(&mut reader)
        .next()
        .transpose()
        .map_err(io_other)?
    {
        return Ok(PrivateKeyDer::Pkcs1(key));
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("no usable private key found in {}", path.display()),
    ))
}

#[derive(Debug)]
struct AaClientCertVerifier(Arc<CryptoProvider>);

impl AaClientCertVerifier {
    fn new() -> Arc<Self> {
        Arc::new(Self(default_provider()))
    }
}

impl ClientCertVerifier for AaClientCertVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct AaServerCertResolver(Arc<CertifiedKey>);

impl AaServerCertResolver {
    fn new(cert: CertificateDer<'static>, key: PrivateKeyDer<'static>) -> io::Result<Arc<Self>> {
        let signing_key = aws_lc_rs::default_provider()
            .key_provider
            .load_private_key(key)
            .map_err(io_other)?;
        Ok(Arc::new(Self(Arc::new(CertifiedKey::new(
            vec![cert],
            signing_key,
        )))))
    }
}

impl rustls::server::ResolvesServerCert for AaServerCertResolver {
    fn resolve(&self, _client_hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }
}

fn io_other<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}
