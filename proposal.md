<!-- Completely separate fs Encoding from compression Encoding -->
<!-- Possibly separate public Encoding (no cfgs) from SupportedEncoding -->

I've been playing around with this, does this API seem reasonable to everybody?

```rust
impl CompressionLayer<_> {
    pub fn encoding_preference<T>(mut self, pref: T) -> Self
    where
        T: Into<EncodingPreference>;
}

pub struct EncodingPreference(/* private fields */);

impl EncodingPreference {
    pub predicate<F>(f: F) -> Self
    where
        F: for<'a> Fn(&'a [(Encoding, QValue)]) -> &'a Encoding + Send + Sync + 'static;
}

impl From<[Encoding; 4]> for EncodingPreference;
impl From<[Encoding; 3]> for EncodingPreference;
impl From<[Encoding; 2]> for EncodingPreference;
impl From<[Encoding; 1]> for EncodingPreference;
impl From<Encoding> for EncodingPreference;

pub enum Encoding {
    Deflate,
    Gzip,
    Brotli,
    Zstd,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct QValue(/* private fields */);
```

That is, for simple cases one would write `.encoding_preference([Encoding::Zstd, Encoding::Gzip])`, and for special cases like @privettoli's, one would write `.encoding_preference(EncodingPreference::predicate(|client_supported_encodings| /* ... */))`.

I'm a little bit uncertain what to do with the previous methods to enable or disable certain encodings, they don't seem all that useful given the more powerful API proposed above. Would anybody be opposed to the methods like `.gzip(enable: bool)`, `.no_gzip()` and such being deprecated / removed in favor of the above API plus one method `.disable_other_encodings()` that disables all encodings that 
