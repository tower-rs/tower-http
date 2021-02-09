use http::HeaderValue;

#[derive(Debug, Clone, Copy)]
pub(crate) struct AcceptEncoding {
    pub(crate) gzip: bool,
    pub(crate) deflate: bool,
    pub(crate) br: bool,
}

impl AcceptEncoding {
    #[allow(dead_code)]
    pub(crate) fn to_header_value(&self) -> Option<HeaderValue> {
        let accept = match (self.gzip(), self.deflate(), self.br()) {
            (true, true, true) => "gzip,deflate,br",
            (true, true, false) => "gzip,deflate",
            (true, false, true) => "gzip,br",
            (true, false, false) => "gzip",
            (false, true, true) => "deflate,br",
            (false, true, false) => "deflate",
            (false, false, true) => "br",
            (false, false, false) => return None,
        };
        Some(HeaderValue::from_static(accept))
    }

    #[allow(dead_code)]
    pub(crate) fn gzip(&self) -> bool {
        #[cfg(any(feature = "decompression-gzip", feature = "compression-gzip"))]
        {
            self.gzip
        }
        #[cfg(not(any(feature = "decompression-gzip", feature = "compression-gzip")))]
        {
            false
        }
    }

    #[allow(dead_code)]
    pub(crate) fn deflate(&self) -> bool {
        #[cfg(any(feature = "decompression-deflate", feature = "compression-deflate"))]
        {
            self.deflate
        }
        #[cfg(not(any(feature = "decompression-deflate", feature = "compression-deflate")))]
        {
            false
        }
    }

    #[allow(dead_code)]
    pub(crate) fn br(&self) -> bool {
        #[cfg(any(feature = "decompression-br", feature = "compression-br"))]
        {
            self.br
        }
        #[cfg(not(any(feature = "decompression-br", feature = "compression-br")))]
        {
            false
        }
    }

    #[allow(dead_code)]
    pub(crate) fn set_gzip(mut self, enable: bool) -> Self {
        self.gzip = enable;
        self
    }

    #[allow(dead_code)]
    pub(crate) fn set_deflate(mut self, enable: bool) -> Self {
        self.deflate = enable;
        self
    }

    #[allow(dead_code)]
    pub(crate) fn set_br(mut self, enable: bool) -> Self {
        self.br = enable;
        self
    }
}

impl Default for AcceptEncoding {
    fn default() -> Self {
        AcceptEncoding {
            gzip: true,
            deflate: true,
            br: true,
        }
    }
}
