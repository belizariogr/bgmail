//! TCP + TLS plumbing shared by the IMAP client (and the TLS policy SMTP
//! mirrors): connect with timeouts, wrap in rustls, upgrade after STARTTLS.
//!
//! The rustls `ClientConfig`s are built once per process (`OnceLock`) because
//! loading the OS trust store is not free; startup never pays for it, only the
//! first connection does.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mail_core::{RemoteError, RemoteErrorKind, RemoteResult};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};

/// Budget for establishing the TCP connection (per resolved address).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Budget for any single blocking read or write once connected.
pub const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// Per-connection transport knobs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConnectOptions {
    /// Skips certificate validation (chain, expiry, host name). Only for local
    /// test servers with self-signed certificates (GreenMail, Proton Bridge on
    /// loopback); the app never sets it for remote hosts.
    pub accept_invalid_certs: bool,
}

/// A client TLS stream over a blocking socket.
pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// The process-wide ring provider; explicit so we never depend on (or race
/// with) rustls' global default provider.
pub fn crypto_provider() -> Arc<CryptoProvider> {
    static PROVIDER: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    PROVIDER
        .get_or_init(|| Arc::new(rustls::crypto::ring::default_provider()))
        .clone()
}

/// Cached rustls client config: the OS verifier, or the accept-anything one
/// when `accept_invalid_certs` is set.
pub fn client_config(options: ConnectOptions) -> RemoteResult<Arc<ClientConfig>> {
    static VERIFIED: OnceLock<Result<Arc<ClientConfig>, String>> = OnceLock::new();
    static INSECURE: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    if options.accept_invalid_certs {
        return Ok(INSECURE
            .get_or_init(|| {
                let provider = crypto_provider();
                Arc::new(
                    ClientConfig::builder_with_provider(provider.clone())
                        .with_safe_default_protocol_versions()
                        .expect("ring supports the default protocol versions")
                        .dangerous()
                        .with_custom_certificate_verifier(Arc::new(AcceptAnyCert { provider }))
                        .with_no_client_auth(),
                )
            })
            .clone());
    }
    VERIFIED
        .get_or_init(|| {
            let provider = crypto_provider();
            let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
                .map_err(|err| err.to_string())?;
            Ok(Arc::new(
                ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .map_err(|err| err.to_string())?
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(verifier))
                    .with_no_client_auth(),
            ))
        })
        .clone()
        .map_err(|message| RemoteError::new(RemoteErrorKind::Tls, message))
}

/// Resolves `host` and connects to the first reachable address, with
/// [`CONNECT_TIMEOUT`] per address and [`IO_TIMEOUT`] on reads/writes.
pub fn connect_tcp(host: &str, port: u16) -> RemoteResult<TcpStream> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|err| network(format!("cannot resolve {host}: {err}")))?;
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(IO_TIMEOUT))
                    .map_err(io_error)?;
                stream
                    .set_write_timeout(Some(IO_TIMEOUT))
                    .map_err(io_error)?;
                // Small command/response protocol: don't let Nagle delay commands.
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(err) => last_error = Some(err),
        }
    }
    Err(match last_error {
        Some(err) => network(format!("cannot connect to {host}:{port}: {err}")),
        None => network(format!("{host} has no addresses")),
    })
}

/// Wraps a connected socket in TLS for `host` and completes the handshake
/// eagerly, so certificate problems surface here as [`RemoteErrorKind::Tls`]
/// instead of on the first command.
pub fn connect_tls(tcp: TcpStream, host: &str, options: ConnectOptions) -> RemoteResult<TlsStream> {
    let config = client_config(options)?;
    let name = ServerName::try_from(host.to_string())
        .map_err(|err| RemoteError::new(RemoteErrorKind::Tls, format!("bad host name: {err}")))?;
    let connection = ClientConnection::new(config, name)
        .map_err(|err| RemoteError::new(RemoteErrorKind::Tls, err.to_string()))?;
    let mut stream = StreamOwned::new(connection, tcp);
    while stream.conn.is_handshaking() {
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(io_error)?;
    }
    Ok(stream)
}

/// Reads one CRLF-terminated line byte by byte. Used before a TLS upgrade,
/// where buffering could swallow the first bytes of the handshake.
pub fn read_line_unbuffered(stream: &mut impl Read) -> io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while !line.ends_with(b"\r\n") {
        if stream.read(&mut byte)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed by server",
            ));
        }
        line.push(byte[0]);
        if line.len() > 8192 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
        }
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

/// IMAP STARTTLS on a fresh plain socket: reads the greeting, sends
/// `STARTTLS` and waits for the tagged `OK`. The caller then calls
/// [`connect_tls`] on the same socket.
pub fn imap_starttls(stream: &mut TcpStream) -> RemoteResult<()> {
    let greeting = read_line_unbuffered(stream).map_err(io_error)?;
    if !greeting.starts_with("* OK") {
        return Err(RemoteError::new(
            RemoteErrorKind::Protocol,
            format!("unexpected greeting: {}", greeting.trim_end()),
        ));
    }
    stream.write_all(b"s0 STARTTLS\r\n").map_err(io_error)?;
    stream.flush().map_err(io_error)?;
    loop {
        let line = read_line_unbuffered(stream).map_err(io_error)?;
        if let Some(status) = line.strip_prefix("s0 ") {
            return if status.to_ascii_uppercase().starts_with("OK") {
                Ok(())
            } else {
                Err(RemoteError::new(
                    RemoteErrorKind::Tls,
                    format!("STARTTLS refused: {}", status.trim_end()),
                ))
            };
        }
    }
}

/// Maps transport errors: TLS failures (rustls wraps them as `InvalidData`
/// with a `rustls::Error` inside) → `Tls`, everything else → `Network`.
pub fn io_error(err: io::Error) -> RemoteError {
    let is_tls = err
        .get_ref()
        .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some());
    if is_tls {
        return RemoteError::new(RemoteErrorKind::Tls, err.to_string());
    }
    let message = match err.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => format!("timed out: {err}"),
        _ => err.to_string(),
    };
    network(message)
}

fn network(message: String) -> RemoteError {
    RemoteError::new(RemoteErrorKind::Network, message)
}

/// Best-effort hard close. Unblocks a thread stuck reading this socket on
/// Linux and macOS. On Windows that read only returns once the peer reacts to
/// our FIN by closing, or at [`IO_TIMEOUT`] (or IDLE's own timeout) if it never
/// does, e.g. a dead network: cancelling is bounded there, not instant.
pub fn shutdown(socket: &TcpStream) {
    let _ = socket.shutdown(Shutdown::Both);
}

/// Accepts any certificate but still checks handshake signatures, so the
/// session is encrypted (just not authenticated). See [`ConnectOptions`].
#[derive(Debug)]
struct AcceptAnyCert {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A tiny rustls server with the self-signed fixture certificate.

    use super::*;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::PrivateKeyDer;
    use rustls::{ServerConfig, ServerConnection};

    /// A loopback port nothing listens on. Tests must not "reserve" a port
    /// by binding `:0` and dropping the listener: another test thread's
    /// server can get the same ephemeral port and then receives the stray
    /// connection (that is what made the OAuth flow test flaky). Port 1 is
    /// outside every OS's ephemeral range, so no test server ever gets it.
    pub const REFUSED_PORT: u16 = 1;

    pub fn server_config() -> Arc<ServerConfig> {
        let cert =
            CertificateDer::from_pem_slice(include_bytes!("../tests/fixtures/test-cert.pem"))
                .unwrap();
        let key = PrivateKeyDer::from_pem_slice(include_bytes!("../tests/fixtures/test-key.pem"))
            .unwrap();
        Arc::new(
            ServerConfig::builder_with_provider(crypto_provider())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .unwrap(),
        )
    }

    pub fn accept_tls(tcp: TcpStream) -> io::Result<StreamOwned<ServerConnection, TcpStream>> {
        let connection = ServerConnection::new(server_config()).map_err(io::Error::other)?;
        let mut stream = StreamOwned::new(connection, tcp);
        while stream.conn.is_handshaking() {
            stream.conn.complete_io(&mut stream.sock)?;
        }
        Ok(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;

    fn insecure() -> ConnectOptions {
        ConnectOptions {
            accept_invalid_certs: true,
        }
    }

    #[test]
    fn options_default_to_verifying() {
        assert!(!ConnectOptions::default().accept_invalid_certs);
    }

    #[test]
    fn configs_are_cached() {
        let a = client_config(insecure()).unwrap();
        let b = client_config(insecure()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let c = client_config(ConnectOptions::default()).unwrap();
        let d = client_config(ConnectOptions::default()).unwrap();
        assert!(Arc::ptr_eq(&c, &d));
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(Arc::ptr_eq(&crypto_provider(), &crypto_provider()));
    }

    #[test]
    fn connect_tcp_sets_timeouts_and_reports_refusal() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let stream = connect_tcp("127.0.0.1", port).unwrap();
        assert_eq!(stream.read_timeout().unwrap(), Some(IO_TIMEOUT));
        assert_eq!(stream.write_timeout().unwrap(), Some(IO_TIMEOUT));
        let err = connect_tcp("127.0.0.1", test_support::REFUSED_PORT).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Network);
        let err = connect_tcp("no-such-host.invalid", 1).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Network);
    }

    fn tls_echo_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for tcp in listener.incoming().flatten() {
                thread::spawn(move || {
                    if let Ok(mut tls) = test_support::accept_tls(tcp) {
                        let mut line = String::new();
                        let mut reader = BufReader::new(&mut tls);
                        if reader.read_line(&mut line).is_ok() {
                            let _ = tls.write_all(line.as_bytes());
                            let _ = tls.flush();
                        }
                    }
                });
            }
        });
        port
    }

    #[test]
    fn connect_tls_with_accept_invalid_certs_talks() {
        let port = tls_echo_server();
        let tcp = connect_tcp("127.0.0.1", port).unwrap();
        let mut tls = connect_tls(tcp, "localhost", insecure()).unwrap();
        tls.write_all(b"ping\r\n").unwrap();
        tls.flush().unwrap();
        let mut line = String::new();
        BufReader::new(&mut tls).read_line(&mut line).unwrap();
        assert_eq!(line, "ping\r\n");
    }

    #[test]
    fn connect_tls_rejects_self_signed_by_default() {
        let port = tls_echo_server();
        let tcp = connect_tcp("127.0.0.1", port).unwrap();
        let err = connect_tls(tcp, "localhost", ConnectOptions::default()).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Tls, "{err}");
    }

    #[test]
    fn read_line_unbuffered_stops_at_crlf() {
        let mut input: &[u8] = b"* OK hi\r\nrest";
        assert_eq!(read_line_unbuffered(&mut input).unwrap(), "* OK hi\r\n");
        assert_eq!(input, b"rest");
        let mut eof: &[u8] = b"partial";
        assert_eq!(
            read_line_unbuffered(&mut eof).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        let long = vec![b'a'; 9000];
        assert_eq!(
            read_line_unbuffered(&mut long.as_slice())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    fn starttls_server(reply: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            let (mut tcp, _) = listener.accept().unwrap();
            tcp.write_all(b"* OK ready\r\n").unwrap();
            let line = read_line_unbuffered(&mut tcp).unwrap();
            assert_eq!(line, "s0 STARTTLS\r\n");
            tcp.write_all(b"* note\r\n").unwrap();
            tcp.write_all(reply.as_bytes()).unwrap();
            if reply.contains("OK") {
                let mut tls = test_support::accept_tls(tcp).unwrap();
                tls.write_all(b"secure\r\n").unwrap();
                tls.flush().unwrap();
            }
        });
        port
    }

    #[test]
    fn imap_starttls_upgrades() {
        let port = starttls_server("s0 OK begin TLS\r\n");
        let mut tcp = connect_tcp("127.0.0.1", port).unwrap();
        imap_starttls(&mut tcp).unwrap();
        let mut tls = connect_tls(tcp, "localhost", insecure()).unwrap();
        assert_eq!(read_line_unbuffered(&mut tls).unwrap(), "secure\r\n");
    }

    #[test]
    fn imap_starttls_refusal_is_tls_error() {
        let port = starttls_server("s0 BAD not now\r\n");
        let mut tcp = connect_tcp("127.0.0.1", port).unwrap();
        let err = imap_starttls(&mut tcp).unwrap_err();
        assert_eq!(err.kind, RemoteErrorKind::Tls);
        assert!(err.message.contains("not now"));
    }

    #[test]
    fn io_error_classifies_tls_timeout_and_network() {
        let tls = io::Error::new(io::ErrorKind::InvalidData, rustls::Error::DecryptError);
        assert_eq!(io_error(tls).kind, RemoteErrorKind::Tls);
        let timeout = io_error(io::Error::from(io::ErrorKind::TimedOut));
        assert_eq!(timeout.kind, RemoteErrorKind::Network);
        assert!(timeout.message.starts_with("timed out"));
        let reset = io_error(io::Error::from(io::ErrorKind::ConnectionReset));
        assert_eq!(reset.kind, RemoteErrorKind::Network);
    }

    #[test]
    fn shutdown_unblocks_a_reader() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = connect_tcp("127.0.0.1", port).unwrap();
        let server = listener.accept().unwrap();
        let clone = client.try_clone().unwrap();
        let reader = thread::spawn(move || {
            let mut buf = [0u8; 1];
            client.read(&mut buf)
        });
        thread::sleep(Duration::from_millis(50));
        shutdown(&clone);
        // See `shutdown`: on Windows the read also needs the peer to close.
        #[cfg(windows)]
        drop(server);
        // Either EOF (Ok(0)) or an error; what matters is that it returns.
        let result = reader.join().unwrap();
        assert!(matches!(result, Ok(0) | Err(_)));
    }
}
