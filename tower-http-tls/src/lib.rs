extern crate futures;
extern crate http;
extern crate rustls;
extern crate tokio_io;
extern crate tokio_rustls;
extern crate tokio_tcp;
extern crate tower_http_util;
extern crate tower_service;
extern crate webpki;
extern crate webpki_roots;

use futures::{Future, Poll};
use http::{
    uri::{Scheme, Uri},
    Version,
};
use rustls::{ClientConfig, Session};
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_rustls::client::TlsStream;
use tokio_tcp::TcpStream;
use tower_http_util::HttpConnection;
use tower_service::Service;
use webpki::DNSNameRef;
use webpki_roots::TLS_SERVER_ROOTS;

const ALPN_H2: &str = "h2";

pub struct TlsConnector {
    config: Arc<ClientConfig>,
}

pub struct TlsConnection {
    inner: TlsStream<TcpStream>,
}

impl TlsConnector {
    pub fn new(config: ClientConfig) -> Self {
        let config = Arc::new(config);
        TlsConnector { config }
    }

    pub fn with_root(h2: bool) -> Self {
        let mut config = ClientConfig::new();
        config
            .root_store
            .add_server_trust_anchors(&TLS_SERVER_ROOTS);

        if h2 {
            config.alpn_protocols.push(Vec::from(&ALPN_H2[..]));
        }

        TlsConnector::new(config)
    }
}

impl<Target> Service<Target> for TlsConnector
where
    Target: AsRef<str> + 'static,
{
    type Response = TlsConnection;
    type Error = Box<std::error::Error + Send + Sync + 'static>;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error> + Send + 'static>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(().into())
    }

    fn call(&mut self, target: Target) -> Self::Future {
        let uri = target.as_ref().parse::<Uri>().unwrap();
        let auth = uri.authority_part().unwrap();
        let host = auth.host();

        let port = if let Some(port) = auth.port_u16() {
            port
        } else {
            if let Some(scheme) = uri.scheme_part() {
                if scheme == &Scheme::HTTP {
                    80
                } else if scheme == &Scheme::HTTPS {
                    443
                } else {
                    panic!("not a valid http scheme")
                }
            } else {
                panic!("No port provided and no scheme provided")
            }
        };

        let addr = (host, port).to_socket_addrs().unwrap().next().unwrap();

        let dns = DNSNameRef::try_from_ascii_str(host).unwrap().to_owned();
        let config = self.config.clone();

        let connect = TcpStream::connect(&addr)
            .and_then(move |io| tokio_rustls::TlsConnector::from(config).connect(dns.as_ref(), io))
            .map(TlsConnection::from)
            .map_err(Into::into);

        Box::new(connect)
    }
}

impl HttpConnection for TlsConnection {
    fn version(&self) -> Version {
        let (_, session) = self.inner.get_ref();
        let negotiated_protocol = session.get_alpn_protocol();

        if Some(ALPN_H2.as_bytes()) == negotiated_protocol.as_ref().map(|x| &**x) {
            Version::HTTP_2
        } else {
            Version::default()
        }
    }

    fn remote_addr(&self) -> std::io::Result<SocketAddr> {
        let (io, _) = self.inner.get_ref();
        io.peer_addr()
    }
}

impl AsyncRead for TlsConnection {}

impl AsyncWrite for TlsConnection {
    fn shutdown(&mut self) -> Poll<(), std::io::Error> {
        self.inner.shutdown()
    }
}

impl std::io::Read for TlsConnection {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl std::io::Write for TlsConnection {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl From<TlsStream<TcpStream>> for TlsConnection {
    fn from(inner: TlsStream<TcpStream>) -> Self {
        TlsConnection { inner }
    }
}
