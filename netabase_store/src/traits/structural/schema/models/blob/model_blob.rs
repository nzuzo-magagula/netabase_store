use crate::keys::ordered::{KeyCodecError, OrderedKeyEncoding};
use crate::traits::structural::schema::models::blob::Blobbable;

/// Addresses one chunk of a blobbed value: the owning model's primary key
/// plus the chunk index. `u64` (not `usize`) so the encoding is identical on
/// every architecture.
///
/// Ordered encoding is `(key, index)` — so a per-key range scan yields a
/// model's chunks contiguously and in order for reassembly.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobChunkKey<K>(pub K, pub u64);

impl<K: OrderedKeyEncoding> OrderedKeyEncoding for BlobChunkKey<K> {
    const MAX_ENCODED_LEN: usize = K::MAX_ENCODED_LEN + 8;

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, KeyCodecError> {
        let mut written = self.0.encode_into(out)?;
        written += self.1.encode_into(&mut out[written..])?;
        Ok(written)
    }

    fn decode(bytes: &[u8]) -> Result<(Self, usize), KeyCodecError> {
        let (key, mut consumed) = K::decode(bytes)?;
        let (index, n) = u64::decode(&bytes[consumed..])?;
        consumed += n;
        Ok((Self(key, index), consumed))
    }
}

pub trait NetabaseModelBlob: Blobbable {}
