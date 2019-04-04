extern crate futures;
extern crate http_body;
extern crate tokio_buf;

use futures::Poll;
use http_body::Body;
use tokio_buf::{BufStream, SizeHint};

struct Mock {
    size_hint: SizeHint,
}

impl BufStream for Mock {
    type Item = ::std::io::Cursor<Vec<u8>>;
    type Error = ();

    fn poll_buf(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        unimplemented!();
    }

    fn size_hint(&self) -> SizeHint {
        self.size_hint.clone()
    }
}

#[test]
fn buf_stream_is_end_stream() {
    let combos = [
        (None, None, false),
        (Some(123), None, false),
        (Some(0), Some(123), false),
        (Some(123), Some(123), false),
        (Some(0), Some(0), true),
    ];

    for &(lower, upper, is_end_stream) in &combos {
        let mut size_hint = SizeHint::new();
        assert_eq!(0, size_hint.lower());
        assert!(size_hint.upper().is_none());

        if let Some(lower) = lower {
            size_hint.set_lower(lower);
        }

        if let Some(upper) = upper {
            size_hint.set_upper(upper);
        }

        let mock = Mock { size_hint };
        assert_eq!(
            is_end_stream,
            mock.is_end_stream(),
            "size_hint = {:?}",
            mock.size_hint
        );
    }
}
