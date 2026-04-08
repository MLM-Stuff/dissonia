/// Vorbis comment metadata shared by Ogg Opus, Ogg Vorbis, Ogg FLAC,
/// and native FLAC VORBIS_COMMENT blocks.
///
/// Field names are case-insensitive per the spec; they are stored as
/// provided and compared in a case-insensitive manner on lookup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VorbisComments {
    vendor: String,
    entries: Vec<(String, String)>,
}

impl VorbisComments {
    /// Creates a new comment block with the given vendor string and no
    /// user comments.
    #[must_use]
    pub fn new(vendor: impl Into<String>) -> Self {
        Self {
            vendor: vendor.into(),
            entries: Vec::new(),
        }
    }

    /// Returns the vendor string.
    #[must_use]
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    /// Replaces the vendor string.
    pub fn set_vendor(&mut self, vendor: impl Into<String>) {
        self.vendor = vendor.into();
    }

    /// Appends a `FIELD=value` comment.  Duplicate field names are
    /// allowed (e.g. multiple `ARTIST` entries).
    pub fn add(&mut self, field: impl Into<String>, value: impl Into<String>) {
        self.entries.push((field.into(), value.into()));
    }

    /// Returns the first value for `field` (case-insensitive), if any.
    #[must_use]
    pub fn get(&self, field: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(field))
            .map(|(_, v)| v.as_str())
    }

    /// Returns every value for `field` (case-insensitive).
    #[must_use]
    pub fn get_all(&self, field: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(field))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// Iterates all `(field, value)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Returns the number of user comment entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when there are no user comment entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Encodes the raw `FIELD=value` strings used in the wire format.
    #[must_use]
    pub fn to_raw_strings(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect()
    }

    /// Encodes the comment block **without** any container framing.
    ///
    /// Layout (all little-endian):
    ///
    /// ```text
    /// vendor_length : u32
    /// vendor_string : [u8; vendor_length]
    /// comment_count : u32
    /// for each comment:
    ///     length : u32
    ///     string : [u8; length]   ("FIELD=value")
    /// ```
    ///
    /// Callers add their own framing — `OpusTags` for Ogg Opus,
    /// a FLAC metadata-block header for native FLAC, etc.
    ///
    /// Returns `None` if any length exceeds `u32`.
    #[must_use]
    pub fn encode(&self) -> Option<Vec<u8>> {
        let vendor_bytes = self.vendor.as_bytes();
        let vendor_len = u32::try_from(vendor_bytes.len()).ok()?;

        let raw = self.to_raw_strings();
        let comment_count = u32::try_from(raw.len()).ok()?;

        let mut capacity: usize = 4 + vendor_bytes.len() + 4;
        for r in &raw {
            capacity = capacity.checked_add(4 + r.len())?;
        }

        let mut buf = Vec::with_capacity(capacity);
        buf.extend_from_slice(&vendor_len.to_le_bytes());
        buf.extend_from_slice(vendor_bytes);
        buf.extend_from_slice(&comment_count.to_le_bytes());

        for r in &raw {
            let bytes = r.as_bytes();
            let len = u32::try_from(bytes.len()).ok()?;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(bytes);
        }

        Some(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_basic_comments() {
        let mut comments = VorbisComments::new("test-vendor");
        comments.add("ARTIST", "Someone");
        comments.add("TITLE", "Something");
        comments.add("artist", "Another");

        assert_eq!(comments.vendor(), "test-vendor");
        assert_eq!(comments.len(), 3);
        assert_eq!(comments.get("ARTIST"), Some("Someone"));
        assert_eq!(comments.get("artist"), Some("Someone"));
        assert_eq!(comments.get_all("artist"), vec!["Someone", "Another"]);
        assert_eq!(comments.get("TITLE"), Some("Something"));
        assert_eq!(comments.get("ALBUM"), None);
    }

    #[test]
    fn encodes_to_expected_binary_layout() {
        let mut comments = VorbisComments::new("v");
        comments.add("A", "1");

        let encoded = comments.encode().unwrap();

        // vendor_len(4) + "v"(1) + count(4) + len(4) + "A=1"(3) = 16
        assert_eq!(encoded.len(), 16);
        assert_eq!(u32::from_le_bytes(encoded[0..4].try_into().unwrap()), 1);
        assert_eq!(&encoded[4..5], b"v");
        assert_eq!(u32::from_le_bytes(encoded[5..9].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(encoded[9..13].try_into().unwrap()), 3);
        assert_eq!(&encoded[13..16], b"A=1");
    }

    #[test]
    fn empty_comments_encode() {
        let comments = VorbisComments::new("x");
        let encoded = comments.encode().unwrap();

        assert_eq!(u32::from_le_bytes(encoded[0..4].try_into().unwrap()), 1);
        assert_eq!(&encoded[4..5], b"x");
        assert_eq!(u32::from_le_bytes(encoded[5..9].try_into().unwrap()), 0);
        assert_eq!(encoded.len(), 9);
    }

    #[test]
    fn iter_preserves_insertion_order() {
        let mut comments = VorbisComments::new("");
        comments.add("B", "2");
        comments.add("A", "1");

        let pairs: Vec<_> = comments.iter().collect();
        assert_eq!(pairs, vec![("B", "2"), ("A", "1")]);
    }
}
