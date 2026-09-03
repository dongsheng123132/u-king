//! Secret-bearing route hand-off between the provider resolver and isolated
//! runtime adapters. This is intentionally a tiny neutral module, not an
//! installer concern and not a provider/runtime dependency.

/// Intentionally no Serialize/Debug: an API key must never be printable by an
/// Action response, progress event, marker, or diagnostic.
pub(crate) struct OpenClaw2ModelRoute {
    pub source_id: String,
    pub source_name: String,
    pub base: String,
    pub model: String,
    pub key: String,
    pub key_source: String,
}
