//! Types and implementations for [`ConnectionInfo`].

use std::{
    borrow::{Borrow, Cow},
    convert::TryFrom,
    fmt,
    net::{IpAddr, SocketAddr},
};

use http::{uri::Scheme, HeaderValue};

/// Information about the source of an incoming request.
///
/// If gateway is not configured to read a `ConnectionInfo` from the request, or the
/// [`peer_ip`](Self::peer_ip) field is [`None`], the [`Forwarded`][] header will specify
/// `for=unknown`.
///
/// This type can store both borrowed and owned data. Typically the data will be borrowed from
/// request extensions, but it may also return owned data in the case where the data is being
/// generated from within a [`Gateway::with_connection_info_fn`] handler.
///
/// # Implementation notes
///
/// The common way to produce this type is through an [`Into<ConnectionInfo>`] impl (or the dual
/// [`From`] impl) on a value that is stored in the request extensions. A few basic implementations
/// are already provided.
///
/// When implementing this on a new type, you will typically want to implement it for `&T`. This is
/// because it will typically be called on borrowed data. However if your type is [`Copy`] then it
/// can implement [`Into<ConnectionInfo>`] directly and an [existing blanket impl][blanket] will
/// provide the expected `&T` impl.
///
/// ## Examples
///
/// Non-[`Copy`] type:
///
/// ```rust
/// use tower_http::gateway::ConnectionInfo;
///
/// struct SocketInfo {
///     ip: std::net::IpAddr,
///     scheme: http::uri::Scheme,
/// }
///
/// impl<'a> From<&'a SocketInfo> for ConnectionInfo<'a> {
///     fn from(info: &'a SocketInfo) -> Self {
///         ConnectionInfo::new()
///             .peer_ip(Some(info.ip))
///             .scheme(Some(&info.scheme))
///     }
/// }
/// ```
///
/// [`Copy`] type:
///
/// ```rust
/// use tower_http::gateway::ConnectionInfo;
///
/// #[derive(Copy, Clone)]
/// struct SocketInfo {
///     ip: std::net::IpAddr,
/// }
///
/// impl From<SocketInfo> for ConnectionInfo<'_> {
///     fn from(info: SocketInfo) -> Self {
///         info.ip.into()
///     }
/// }
///
/// let _ = ConnectionInfo::from(&SocketInfo { ip: std::net::Ipv4Addr::LOCALHOST.into() });
/// ```
///
/// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
/// [`Gateway::with_connection_info_fn`]: fn@super::Gateway::with_connection_info_fn
/// [blanket]: #impl-From%3C%26%27_%20C%3E
#[derive(Clone, Debug, Default)]
pub struct ConnectionInfo<'a> {
    /// The peer IP address for the connection, a generated token obfuscating the peer IP address,
    /// or `None` if the IP address is unknown.
    ///
    /// This is used by the `for` directive of the [`Forwarded`][] header along with
    /// [`self.peer_port`](#structfield.peer_port), and by the [`X-Forwarded-For`][] header if it
    /// is [`NodeIdentifier::Value`].
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    /// [`X-Forwarded-For`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-For
    pub peer_ip: Option<NodeIdentifier<'a, IpAddr>>,
    /// The port number associated with the peer IP address for the connection, a generated token
    /// obfuscating the peer port, or `None` if the port number is unknown or irrelevant.
    ///
    /// This is used by the `for` directive of the [`Forwarded`][] header along with
    /// [`self.peer_ip`](#structfield.peer_ip).
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub peer_port: Option<NodeIdentifier<'a, u16>>,

    /// The local IP address for the connection, a generated token obfuscating the local IP
    /// address, or `None` if the IP address is unknown or irrelevant.
    ///
    /// This is used by the `by` directive of the [`Forwarded`][] header along with
    /// [`self.local_port`](#structfield.local_port).
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub local_ip: Option<NodeIdentifier<'a, IpAddr>>,
    /// The port number associated with the local IP address for the connection, a generated token
    /// obfuscating the local port, or `None` if the port number is unknown or irrelevant.
    ///
    /// This is used by the `by` directive of the [`Forwarded`][] header along with
    /// [`self.local_ip`](#structfield.local_ip).
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub local_port: Option<NodeIdentifier<'a, u16>>,

    /// The scheme for the connection, typically `http` or `https`.
    pub scheme: Option<Cow<'a, Scheme>>,

    /// The [`received-by`][received-by] portion of the [`Via`][] header.
    ///
    /// If [`Some`], this value will be used to construct the [`Via`][] header. If [`None`],
    /// [`self.local_ip`][] and [`self.local_port`][] will be used instead. If [`self.local_ip`][]
    /// is also [`None`] then the [`Via`][] header will be left unmodified.
    ///
    /// This field, if set, must match the ABNF syntax
    /// <code>[received-by][] [ [RWS][] [comment][] ]</code> as specified by the [`Via`][] header.
    /// This syntax is not validated by this module, but a failure to match this syntax this may
    /// cause downstream issues with the forwarded request.
    ///
    /// [`Via`]: https://httpwg.org/specs/rfc7230.html#header.via "RFC 7230, Section 5.7.1. Via"
    /// [`self.local_ip`]: #structfield.local_ip
    /// [`self.local_port`]: #structfield.local_port
    /// [received-by]: https://httpwg.org/specs/rfc7230.html#header.via
    /// [RWS]: https://httpwg.org/specs/rfc7230.html#rule.RWS
    /// [comment]: https://httpwg.org/specs/rfc7230.html#rule.comment
    pub via_received_by: Option<Cow<'a, HeaderValue>>,

    /// The [`received-protocol`][] portion of the [`Via`][] header.
    ///
    /// If [`Some`], this value will be used to construct the [`Via`][] header. If [`None`], the
    /// protocol portion of the constructed [`Via`][] header (if any) will be derived from the
    /// request's [`http::Version`].
    ///
    /// This field, if set, must match the ABNF syntax
    /// <code>[ [protocol-name][] &quot;/&quot; ] [protocol-version][]</code> as specified by the
    /// [`Via`][] header. This syntax is not validated by this module, but a failure to match this
    /// syntax this may cause downstream issues with the forwarded request.
    ///
    /// [`received-protocol`]: https://httpwg.org/specs/rfc7230.html#header.via
    /// [`Via`]: https://httpwg.org/specs/rfc7230.html#header.via "RFC 7230, Section 5.7.1. Via"
    /// [protocol-name]: https://httpwg.org/specs/rfc7230.html#header.upgrade
    /// [protocol-version]: https://httpwg.org/specs/rfc7230.html#header.upgrade
    pub via_protocol: Option<Cow<'a, HeaderValue>>,
}

impl<'a> ConnectionInfo<'a> {
    /// Returns a new `ConnectionInfo`.
    ///
    /// All fields are [`None`].
    pub const fn new() -> Self {
        Self {
            peer_ip: None,
            peer_port: None,
            local_ip: None,
            local_port: None,
            scheme: None,
            via_received_by: None,
            via_protocol: None,
        }
    }

    /// Sets the peer node identifier (IP address or generated token).
    ///
    /// This corresponds to the `for` directive in the [`Forwarded`][] header along with
    /// [`self.peer_port`](Self::peer_port), or the value of the [`X-Forwarded-For`][] header.
    ///
    /// `ip` may be an IP address or an obfuscated identifier. See [`ObfuscatedIdentifier`] for
    /// details on the identifier format. If an [`ObfuscatedIdentifier`] is used then the
    /// [`X-Forwarded-For`][] header will not be set or modified.
    ///
    /// If [`None`] (the default) the [`Forwarded`][] header will specify `for=unknown`.
    ///
    /// Also see [`obfuscated_peer_ip`](Self::obfuscated_peer_ip) to work around generic inference
    /// issues caused when trying to call e.g. `peer_ip(Some(token.try_into()?))`.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    /// [`X-Forwarded-For`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-For
    pub fn peer_ip(self, ip: Option<impl Into<NodeIdentifier<'a, IpAddr>>>) -> Self {
        Self {
            peer_ip: ip.map(Into::into),
            ..self
        }
    }

    /// Ssets the peer node identifier to a generated token.
    ///
    /// This corresponds to the `for` directive in the [`Forwarded`][] header along with
    /// [`self.peer_port`](Self::peer_port), or the value of the [`X-Forwarded-For`][] header.
    ///
    /// This is equivalent to calling [`peer_ip`](Self::peer_ip) with the results of
    /// `ip.try_into()` but works around generic inference issues. Note that calling
    /// `obfuscated_peer_ip` with [`None`] will clear any previously-set
    /// [`peer_ip`](Self::peer_ip).
    ///
    /// See [`peer_ip`](Self::peer_ip) for more details.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    /// [`X-Forwarded-For`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-For
    pub fn obfuscated_peer_ip(self, ip: Option<ObfuscatedIdentifier<'a>>) -> Self {
        self.peer_ip(ip)
    }

    /// Sets the peer port identifier (port number or generated token).
    ///
    /// This corresponds to the `for` directive in the [`Forwarded`][] header along with
    /// [`self.peer_ip`](Self::peer_ip).
    ///
    /// `port` may be a port number or an obfuscated identifier. See [`ObfuscatedIdentifier`] for
    /// details on the identifier format.
    ///
    /// Also see [`obfuscated_peer_port`](Self::obfuscated_peer_port) to work around generic
    /// inference issues caused when trying to call e.g. `peer_port(Some(token.try_into()?))`.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn peer_port(self, port: Option<impl Into<NodeIdentifier<'a, u16>>>) -> Self {
        Self {
            peer_port: port.map(Into::into),
            ..self
        }
    }

    /// Sets the peer port identifier to a generated token.
    ///
    /// This corresponds to the `for` directive in the [`Forwarded`][] header along with
    /// [`self.peer_ip`](Self::peer_ip).
    ///
    /// This is equivalent to calling [`peer_port`](Self::peer_port) with the results of
    /// `port.try_into()` but works around generic inference issues. Note that calling
    /// `obfuscated_peer_port` with [`None`] will clear any previously-set
    /// [`peer_port`](Self::peer_port).
    ///
    /// See [`peer_port`](Self::peer_port) for more details.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn obfuscated_peer_port(self, port: Option<ObfuscatedIdentifier<'a>>) -> Self {
        self.peer_port(port)
    }

    /// Sets the peer node and port identifiers to an IP address and port.
    ///
    /// This correponds to the `for` directive in the [`Forwarded`][] header.
    ///
    /// This is equivalent to calling both [`peer_ip`](Self::peer_ip) and
    /// [`peer_port`](Self::peer_port) using the IP and port from the [`SocketAddr`].
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn peer_addr(self, addr: Option<SocketAddr>) -> Self {
        self.peer_ip(addr.map(|a| a.ip()))
            .peer_port(addr.map(|a| a.port()))
    }

    /// Sets the local interface (IP address or generated token).
    ///
    /// This corresponds to the `by` directive in the [`Forwarded`][] header along with
    /// [`self.local_port`](Self::local_port).
    ///
    /// `ip` is the interface at which the request came into the proxy server. This may be an IP
    /// address or a obfuscated identifier. See [`ObfuscatedIdentifier`] for details on the
    /// identifier format.
    ///
    /// If `local_ip` is [`None`] but [`local_port`](Self::local_port) is [`Some`] the
    /// [`Forwarded`][] header will specify `by="unknown:<port>"`. If both are [`None`] the `by`
    /// directive will be omitted.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn local_ip(self, ip: Option<impl Into<NodeIdentifier<'a, IpAddr>>>) -> Self {
        Self {
            local_ip: ip.map(Into::into),
            ..self
        }
    }

    /// Ssets the local interface to a generated token.
    ///
    /// This corresponds to the `by` directive in the [`Forwarded`][] header along with
    /// [`self.local_port`](Self::local_port).
    ///
    /// This is equivalent to calling [`local_ip`](Self::local_ip) with the results of
    /// `ip.try_into()` but works around generic inference issues. Note that calling
    /// `obfuscated_local_ip` with [`None`] will clear any previously-set
    /// [`local_ip`](Self::local_ip).
    ///
    /// See [`local_ip`](Self::local_ip) for more details.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn obfuscated_local_ip(self, ip: Option<ObfuscatedIdentifier<'a>>) -> Self {
        self.local_ip(ip)
    }

    /// Sets the local port identifier (port number or generated token).
    ///
    /// This corresponds to the `by` directive in the [`Forwarded`][] header along with
    /// [`self.local_ip`](Self::local_ip).
    ///
    /// `port` is the port for the interface at which the request came into the proxy server. This
    /// may be a port number or an obfuscated identifier. See [`ObfuscatedIdentifier`] for details
    /// on the identifier format.
    ///
    /// If [`local_ip`](Self::local_ip) is [`None`] but `local_port` is [`Some`] the
    /// [`Forwarded`][] header will specify `by="unknown:<port>"`. If both are [`None`] the `by`
    /// directive will be omitted.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn local_port(self, port: Option<impl Into<NodeIdentifier<'a, u16>>>) -> Self {
        Self {
            local_port: port.map(Into::into),
            ..self
        }
    }

    /// Sets the local port identifier to a generated token.
    ///
    /// This corresponds to the `by` directive in the [`Forwarded`][] header along with
    /// [`self.local_ip`](Self::local_ip).
    ///
    /// This is equivalent to calling [`local_port`](Self::local_port) with the results of
    /// `port.try_into()` but works around generic inference issues. Note that calling
    /// `obfuscated_local_port` with [`None`] will clear any previously-set
    /// [`local_port`](Self::local_port).
    ///
    /// See [`local_port`](Self::local_port) for more details.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn obfuscated_local_port(self, port: Option<ObfuscatedIdentifier<'a>>) -> Self {
        self.local_port(port)
    }

    /// Sets the local node and port identifiers to an IP address and port.
    ///
    /// This correponds to the `by` directive in the [`Forwarded`][] header.
    ///
    /// This is equivalent to calling both [`local_ip`](Self::local_ip) and
    /// [`local_port`](Self::local_port) using the IP and port from the [`SocketAddr`].
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    pub fn local_addr(self, addr: Option<SocketAddr>) -> Self {
        self.local_ip(addr.map(|a| a.ip()))
            .local_port(addr.map(|a| a.port()))
    }

    /// Sets the protocol scheme used by the client connection.
    ///
    /// This corresponds to the `proto` directive in the [`Forwarded`][] header, or the value of
    /// the [`X-Forwarded-Proto`][] header.
    ///
    /// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
    /// [`X-Forwarded-Proto`]:
    ///     https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/X-Forwarded-Proto
    pub fn scheme(self, scheme: Option<impl IntoCow<'a, Scheme>>) -> Self {
        Self {
            scheme: scheme.map(IntoCow::into_cow),
            ..self
        }
    }

    /// Sets the `received-by` portion of the [`Via`][] header.
    ///
    /// If [`Some`], this value will be used to construct the [`Via`][] header. If [`None`],
    /// [`self.local_ip`][] and [`self.local_port`][] will be used instead. If [`self.local_ip`][]
    /// is also [`None`] then the [`Via`][] header will be left unmodified.
    ///
    /// This value, if set, must match the ABNF syntax
    /// <code>[received-by][] [ [RWS][] [comment][] ]</code> as specified by the [`Via`][] header.
    /// This syntax is not validated by this module, but a failure to match this syntax this may
    /// cause downstream issues with the forwarded request.
    ///
    /// [`Via`]: https://httpwg.org/specs/rfc7230.html#header.via "RFC 7230, Section 5.7.1. Via"
    /// [`self.local_ip`]: fn@Self::local_ip
    /// [`self.local_port`]: fn@Self::local_port
    /// [received-by]: https://httpwg.org/specs/rfc7230.html#header.via
    /// [RWS]: https://httpwg.org/specs/rfc7230.html#rule.RWS
    /// [comment]: https://httpwg.org/specs/rfc7230.html#rule.comment
    pub fn via_received_by(self, via_received_by: Option<impl IntoCow<'a, HeaderValue>>) -> Self {
        Self {
            via_received_by: via_received_by.map(IntoCow::into_cow),
            ..self
        }
    }

    /// Sets the [`received-protocol`][] portion of the [`Via`][] header.
    ///
    /// If [`Some`], this value will be used to construct the [`Via`][] header. If [`None`], the
    /// protocol portion of the constructed [`Via`][] header (if any) will be derived from the
    /// request's [`http::Version`].
    ///
    /// This value, if set, must match the ABNF syntax
    /// <code>[ [protocol-name][] &quot;/&quot; ] [protocol-version][]</code> as specified by the
    /// [`Via`][] header. This syntax is not validated by this module, but a failure to match this
    /// syntax this may cause downstream issues with the forwarded request.
    ///
    /// [`received-protocol`]: https://httpwg.org/specs/rfc7230.html#header.via
    /// [`Via`]: https://httpwg.org/specs/rfc7230.html#header.via "RFC 7230, Section 5.7.1. Via"
    /// [protocol-name]: https://httpwg.org/specs/rfc7230.html#header.upgrade
    /// [protocol-version]: https://httpwg.org/specs/rfc7230.html#header.upgrade
    pub fn via_protocol(self, via_protocol: Option<impl IntoCow<'a, HeaderValue>>) -> Self {
        Self {
            via_protocol: via_protocol.map(IntoCow::into_cow),
            ..self
        }
    }
}

impl From<SocketAddr> for ConnectionInfo<'_> {
    /// Converts from [`SocketAddr`] to `ConnectionInfo`.
    ///
    /// The resulting `ConnectionInfo` sets its [`peer_ip`](ConnectionInfo::peer_ip) and
    /// [`peer_port`](ConnectionInfo::peer_port) from the `SocketAddr`. Convert `addr.ip()` instead
    /// if you don't want the peer port set.
    ///
    /// This is equivalent to
    /// `ConnectionInfo::new().peer_ip(Some(addr.ip())).peer_port(Some(addr.port()))`.
    fn from(addr: SocketAddr) -> Self {
        Self::new()
            .peer_ip(Some(addr.ip()))
            .peer_port(Some(addr.port()))
    }
}

impl From<IpAddr> for ConnectionInfo<'_> {
    /// Converts from [`IpAddr`] to `ConnectionInfo`.
    ///
    /// The resulting `ConnctionInfo` sets its [`peer_ip`](ConnectionInfo::peer_ip) from the
    /// `IpAddr`.
    ///
    /// This is equivalent to `ConnectionInfo::new().peer_ip(Some(addr))`.
    fn from(addr: IpAddr) -> Self {
        Self::new().peer_ip(Some(addr))
    }
}

impl From<()> for ConnectionInfo<'_> {
    fn from(_: ()) -> Self {
        Self::new()
    }
}

impl<'a, C: Copy> From<&C> for ConnectionInfo<'a>
where
    C: Into<ConnectionInfo<'a>>,
{
    fn from(c: &C) -> Self {
        (*c).into()
    }
}

impl<'a, C> From<Option<C>> for ConnectionInfo<'a>
where
    C: Into<ConnectionInfo<'a>>,
{
    fn from(c: Option<C>) -> Self {
        c.map(Into::into).unwrap_or_default()
    }
}

impl<'a, 'b: 'a> From<&'a ConnectionInfo<'b>> for ConnectionInfo<'a> {
    fn from(info: &'a ConnectionInfo<'b>) -> Self {
        Self {
            peer_ip: info.peer_ip.as_ref().map(Into::into),
            peer_port: info.peer_port.as_ref().map(Into::into),
            local_ip: info.local_ip.as_ref().map(Into::into),
            local_port: info.local_port.as_ref().map(Into::into),
            scheme: info.scheme.as_ref().map(IntoCow::into_cow),
            via_received_by: info.via_received_by.as_ref().map(IntoCow::into_cow),
            via_protocol: info.via_protocol.as_ref().map(IntoCow::into_cow),
        }
    }
}

/// An obfuscated identifier for use with [`ConnectionInfo`].
///
/// This type represents an obfuscated node identifier (name/address or port) for use with the
/// [`Forwarded`][] header. It guarantees that the contained string matches the ABNF syntax:
///
/// ```abnf
/// obfnode = "_" 1*(ALPHA / DIGIT / "." / "_" / "-")
/// ```
///
/// See [RFC 7239 §6 Node Identifiers][RFC7239§6] for more information.
///
/// [RFC7239§6]: https://www.rfc-editor.org/rfc/rfc7239#section-6 "RFC 7239, Section 6. Node
///     Identifiers"
/// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObfuscatedIdentifier<'a> {
    inner: Cow<'a, str>,
}

impl<'a> ObfuscatedIdentifier<'a> {
    /// Wraps a value in `ObfuscatedIdentifier`, or returns [`Err`] if the value doesn't match the
    /// required ABNF syntax.
    ///
    /// The value must match the ABNF syntax:
    ///
    /// ```abnf
    /// obfnode = "_" 1*(ALPHA / DIGIT / "." / "_" / "-")
    /// ```
    ///
    /// See [RFC 7239 §6 Node Identifiers][RFC7239§6] for more information.
    ///
    /// [RFC7239§6]: https://www.rfc-editor.org/rfc/rfc7239#section-6
    pub fn try_from<T: Into<Cow<'a, str>>>(inner: T) -> Result<Self, InvalidObfuscatedIdentifier> {
        let inner = inner.into();
        let mut iter = inner.as_ref().bytes();
        (iter.next() == Some(b'_')
            && iter.len() > 0 // is_empty is still experimental
            && iter.all(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-')))
        .then(|| Self { inner })
        .ok_or(InvalidObfuscatedIdentifier { _private: () })
    }

    /// Returns a string slice representing the identifier.
    pub fn as_str(&self) -> &str {
        self.inner.as_ref()
    }
}

impl std::ops::Deref for ObfuscatedIdentifier<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl AsRef<str> for ObfuscatedIdentifier<'_> {
    fn as_ref(&self) -> &str {
        self.inner.as_ref()
    }
}

impl TryFrom<String> for ObfuscatedIdentifier<'_> {
    type Error = InvalidObfuscatedIdentifier;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from(s)
    }
}

impl<'a> TryFrom<&'a String> for ObfuscatedIdentifier<'a> {
    type Error = InvalidObfuscatedIdentifier;

    fn try_from(s: &'a String) -> Result<Self, Self::Error> {
        Self::try_from(s)
    }
}

impl<'a> TryFrom<&'a str> for ObfuscatedIdentifier<'a> {
    type Error = InvalidObfuscatedIdentifier;

    fn try_from(s: &'a str) -> Result<Self, Self::Error> {
        Self::try_from(s)
    }
}

impl<'a> TryFrom<Cow<'a, str>> for ObfuscatedIdentifier<'a> {
    type Error = InvalidObfuscatedIdentifier;

    fn try_from(cow: Cow<'a, str>) -> Result<Self, Self::Error> {
        Self::try_from(cow)
    }
}

impl<'a> From<&'a ObfuscatedIdentifier<'_>> for ObfuscatedIdentifier<'a> {
    fn from(ident: &'a ObfuscatedIdentifier<'_>) -> Self {
        ObfuscatedIdentifier {
            inner: ident.inner.as_ref().into(),
        }
    }
}

impl fmt::Debug for ObfuscatedIdentifier<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}

impl fmt::Display for ObfuscatedIdentifier<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, f)
    }
}

/// A possible error when converting an [`ObfuscatedIdentifier`] from an underlying type.
///
/// This error is returned when the value being converted into [`ObfuscatedIdentifier`] does not
/// match the documented ABNF syntax. See [`ObfuscatedIdentifier`] for more details.
pub struct InvalidObfuscatedIdentifier {
    _private: (),
}

impl fmt::Debug for InvalidObfuscatedIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InvalidObfuscatedIdentifier").finish()
    }
}

impl fmt::Display for InvalidObfuscatedIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("failed to parse obfuscated identifier")
    }
}

impl std::error::Error for InvalidObfuscatedIdentifier {}

/// Helper trait used by [`ConnectionInfo`] for types that can convert to [`Cow<'a, B>`](Cow).
///
/// This is a workaround for the fact that [`Cow<'a, B>`](Cow) does not implement [`From<B>`] or
/// [`From<&'a B>`](From) for arbitrary <code>B: [Clone]</code> types.
pub trait IntoCow<'a, B: ?Sized + 'a>
where
    B: ToOwned,
{
    /// Performs the conversion.
    fn into_cow(self) -> Cow<'a, B>;
}

impl<'a, B: ?Sized + 'a> IntoCow<'a, B> for &'a B
where
    B: ToOwned,
{
    fn into_cow(self) -> Cow<'a, B> {
        Cow::Borrowed(self)
    }
}

// This impl must be expressed in terms of Clone instead of being implemented for <B as
// ToOwned>::Owned, as the existence `&_: Clone` would make a `<B as ToOwned>::Owned` impl conflict
// with the above `&'a B` impl.
impl<'a, B: Clone + 'a> IntoCow<'a, B> for B {
    fn into_cow(self) -> Cow<'a, B> {
        Cow::Owned(self)
    }
}

impl<'a, 'b: 'a, B: ToOwned> IntoCow<'a, B> for &'a Cow<'b, B> {
    /// Reborrows the [`Cow<'b, B>`] into [`Cow::Borrowed`].
    fn into_cow(self) -> Cow<'a, B> {
        match *self {
            Cow::Borrowed(b) => Cow::Borrowed(b),
            Cow::Owned(ref o) => Cow::Borrowed(o.borrow()),
        }
    }
}

/// A possibly-obfuscated node identifier.
///
/// This represents an IP address or port, or a generated token that obfuscates the real address or
/// port.
///
/// This is used by [`ConnectionInfo`] and the [`Forwarded`][] header.
///
/// [`Forwarded`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Forwarded
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeIdentifier<'a, T> {
    /// An un-obfuscated IP address or port.
    Value(T),
    /// A generated token obfuscating the real address or port.
    Obfuscated(ObfuscatedIdentifier<'a>),
}

impl<'a, T> NodeIdentifier<'a, T> {
    /// Converts from `NodeIdentifier<T>` to [`Option<T>`].
    ///
    /// Converts `self` into an [`Option<T>`], consuming `self`, and discarding the obfuscated
    /// identifier, if any.
    pub fn exposed(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Obfuscated(_) => None,
        }
    }

    /// Converts from `NodeIdentifier<_>` to [`Option<ObfuscatedIdentifier>`].
    ///
    /// Converts `self` into an [`Option<ObfuscatedIdentifier>`], consuming `self`, and discarding
    /// the un-obfuscated IP address or port, if any.
    pub fn obfuscated(self) -> Option<ObfuscatedIdentifier<'a>> {
        match self {
            Self::Value(_) => None,
            Self::Obfuscated(obf) => Some(obf),
        }
    }

    /// Converts from `&NodeIdentifier<T>` to `NodeIdentifier<&T>`.
    ///
    /// Produces a new `NodeIdentifier`, containing a reference into the original, leaving the
    /// original in place.
    pub fn as_ref(&self) -> NodeIdentifier<&T> {
        match self {
            Self::Value(val) => NodeIdentifier::Value(val),
            Self::Obfuscated(obf) => NodeIdentifier::Obfuscated(obf.into()),
        }
    }

    /// MAps a `NodeIdentifier<'a, T>` to `NodeIdentifier<'a, U>` by applying a function to a
    /// contained value.
    pub fn map_exposed<U, F>(self, f: F) -> NodeIdentifier<'a, U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            Self::Value(val) => NodeIdentifier::Value(f(val)),
            Self::Obfuscated(obf) => NodeIdentifier::Obfuscated(obf),
        }
    }
}

impl<'a, T: Copy> NodeIdentifier<'a, &'_ T> {
    /// Maps a `NodeIdentifier<'a, &T>` to a `NodeIdentifier<'a, T>` by copying the contained
    /// value.
    pub fn copied(self) -> NodeIdentifier<'a, T> {
        self.map_exposed(|&x| x)
    }
}

impl<T: Into<IpAddr>> From<T> for NodeIdentifier<'_, IpAddr> {
    fn from(value: T) -> Self {
        NodeIdentifier::Value(value.into())
    }
}

impl From<u16> for NodeIdentifier<'_, u16> {
    fn from(port: u16) -> Self {
        NodeIdentifier::Value(port)
    }
}

impl<'a, T> From<ObfuscatedIdentifier<'a>> for NodeIdentifier<'a, T> {
    fn from(obf: ObfuscatedIdentifier<'a>) -> Self {
        NodeIdentifier::Obfuscated(obf)
    }
}

impl<'a, T> From<&'a ObfuscatedIdentifier<'_>> for NodeIdentifier<'a, T> {
    fn from(obf: &'a ObfuscatedIdentifier<'_>) -> Self {
        NodeIdentifier::Obfuscated(obf.into())
    }
}

impl<'a, T: Copy> From<&'a NodeIdentifier<'_, T>> for NodeIdentifier<'a, T> {
    fn from(maybe: &'a NodeIdentifier<'_, T>) -> Self {
        match maybe {
            NodeIdentifier::Value(val) => NodeIdentifier::Value(*val),
            NodeIdentifier::Obfuscated(obf) => NodeIdentifier::Obfuscated(obf.into()),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for NodeIdentifier<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(x) => fmt::Debug::fmt(x, f),
            Self::Obfuscated(x) => fmt::Debug::fmt(x, f),
        }
    }
}

impl<T: fmt::Display> fmt::Display for NodeIdentifier<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(x) => fmt::Display::fmt(x, f),
            Self::Obfuscated(x) => fmt::Display::fmt(x, f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_parsing() {
        assert!(ObfuscatedIdentifier::try_from("").is_err());
        assert!(ObfuscatedIdentifier::try_from("_").is_err());
        assert!(ObfuscatedIdentifier::try_from("a").is_err());
        assert!(ObfuscatedIdentifier::try_from("_a").is_ok());
        assert!(ObfuscatedIdentifier::try_from("_Z").is_ok());
        assert!(ObfuscatedIdentifier::try_from("_1").is_ok());
        assert!(ObfuscatedIdentifier::try_from("__").is_ok());
        assert!(ObfuscatedIdentifier::try_from("_-").is_ok());
        assert!(ObfuscatedIdentifier::try_from("unknown").is_err());
        assert!(ObfuscatedIdentifier::try_from("_hidden").is_ok());
        assert!(ObfuscatedIdentifier::try_from("_1234").is_ok());
        assert!(ObfuscatedIdentifier::try_from("_a.b-c_42").is_ok());
        assert!(ObfuscatedIdentifier::try_from("_a b").is_err());
        assert!(ObfuscatedIdentifier::try_from("_a b").is_err());
    }
}
