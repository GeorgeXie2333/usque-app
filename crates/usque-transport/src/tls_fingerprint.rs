//! Fixed Chrome 155 TLS preferences; identity and SPKI verification stay with the caller.
//! Reference: https://github.com/refraction-networking/utls/blob/ff1b50fbbe9a6dff1dcb1cfc0493bd5b0f073f67/u_parrots.go
use boring::error::ErrorStack;
use boring::ssl::{
    CertificateCompressionAlgorithm, CertificateCompressor, SslContextBuilder, SslInfoCallbackMode,
    SslRef, SslSignatureAlgorithm, SslVersion,
};
use foreign_types::ForeignTypeRef;
use std::io::Write;
use usque_core::TlsFingerprint;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Carrier {
    H2,
    H3,
}
const H2_ALPN: &[u8] = b"\x02h2\x08http/1.1";
const H3_ALPN: &[u8] = b"\x02h3";
const CHROME_CIPHERS: &str = concat!(
    "ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:",
    "ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:",
    "ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305:",
    "ECDHE-RSA-AES128-SHA:ECDHE-RSA-AES256-SHA:",
    "AES128-GCM-SHA256:AES256-GCM-SHA384:AES128-SHA:AES256-SHA"
);

pub(crate) fn configure_context(
    builder: &mut SslContextBuilder,
    profile: TlsFingerprint,
    carrier: Carrier,
    #[cfg(test)] probe: Option<crate::chrome_fingerprint_tests::Probe>,
) -> Result<(), ErrorStack> {
    if profile == TlsFingerprint::Chrome155 {
        // set_cipher_list only controls TLS <= 1.2. Pin the TLS 1.3 order and
        // ECH GREASE AEAD independently of AES acceleration on this device.
        // SAFETY: builder exclusively owns a live SSL_CTX before SSL creation.
        // The pinned C adapter only changes this context's handshake preference;
        // cryptographic CPU dispatch continues to use the real hardware flags.
        unsafe { boring_sys::USQUE_SSL_CTX_set_aes_cipher_preference(builder.as_ptr()) };
        // SAFETY: this context is exclusively owned and has no SSL connections.
        // Wire additions do not relax supported peer signatures or SPKI checks.
        unsafe { boring_sys::USQUE_SSL_CTX_set_chrome155_extensions(builder.as_ptr(), 1) };
        builder.set_min_proto_version(Some(match carrier {
            Carrier::H2 => SslVersion::TLS1_2,
            Carrier::H3 => SslVersion::TLS1_3,
        }))?;
        builder.set_max_proto_version(Some(SslVersion::TLS1_3))?;
        builder.set_cipher_list(CHROME_CIPHERS)?;
        builder.set_curves_list("X25519MLKEM768:X25519:P-256:P-384")?;
        builder.set_verify_algorithm_prefs(&[
            SslSignatureAlgorithm::ECDSA_SECP256R1_SHA256,
            SslSignatureAlgorithm::RSA_PSS_RSAE_SHA256,
            SslSignatureAlgorithm::RSA_PKCS1_SHA256,
            SslSignatureAlgorithm::ECDSA_SECP384R1_SHA384,
            SslSignatureAlgorithm::RSA_PSS_RSAE_SHA384,
            SslSignatureAlgorithm::RSA_PKCS1_SHA384,
            SslSignatureAlgorithm::RSA_PSS_RSAE_SHA512,
            SslSignatureAlgorithm::RSA_PKCS1_SHA512,
        ])?;
        builder.set_grease_enabled(true);
        builder.set_permute_extensions(true);
        builder.enable_ocsp_stapling();
        builder.enable_signed_cert_timestamps();
        builder.add_certificate_compression_algorithm(BrotliDecoder)?;
        builder.set_alpn_protos(match carrier {
            Carrier::H2 => H2_ALPN,
            Carrier::H3 => H3_ALPN,
        })?;
    }
    #[cfg(test)]
    if let Some(probe) = &probe {
        probe.observe_messages(builder)?;
    }
    // QUIC owns SSL creation; the infallible ECH GREASE switch is set before
    // its first ClientHello. H2's fallible ALPS setup is done before dialing TLS.
    let needs_callback = profile == TlsFingerprint::Chrome155 && carrier == Carrier::H3;
    #[cfg(test)]
    let needs_callback = needs_callback || probe.is_some();
    if !needs_callback {
        return Ok(());
    }
    builder.set_info_callback(move |ssl, mode, _| {
        if profile == TlsFingerprint::Chrome155
            && carrier == Carrier::H3
            && mode == SslInfoCallbackMode::HANDSHAKE_START
        {
            ssl.set_enable_ech_grease(true);
        }
        #[cfg(test)]
        if mode == SslInfoCallbackMode::HANDSHAKE_DONE
            && let Some(probe) = &probe
        {
            probe.observe_tls_done(ssl);
        }
    });
    Ok(())
}

pub(crate) fn configure_h2_connection(
    ssl: &mut SslRef,
    profile: TlsFingerprint,
) -> Result<(), ErrorStack> {
    if profile == TlsFingerprint::Default {
        return Ok(());
    }
    ssl.set_enable_ech_grease(true);
    // SAFETY: ssl is exclusively borrowed and alive; BoringSSL copies both
    // slices during the call. No pointer is retained by Rust or the peer.
    let ok = unsafe {
        boring_sys::SSL_add_application_settings(ssl.as_ptr(), b"h2".as_ptr(), 2, b"".as_ptr(), 0)
    };
    if ok != 1 {
        return Err(ErrorStack::get());
    }
    Ok(())
}

struct BrotliDecoder;

#[cfg(test)]
pub(crate) fn decompress_probe_certificate(input: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    BrotliDecoder.decompress(input, &mut output).ok()?;
    Some(output)
}

#[derive(Default)]
struct BrotliBytes(Vec<u8>);

impl brotli_decompressor::SliceWrapper<u8> for BrotliBytes {
    fn slice(&self) -> &[u8] {
        &self.0
    }
}

impl brotli_decompressor::SliceWrapperMut<u8> for BrotliBytes {
    fn slice_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

#[derive(Default)]
struct BrotliByteAllocator<const BYTE_BUDGET: usize> {
    allocated: usize,
    #[cfg(test)]
    peak_allocated: usize,
    #[cfg(test)]
    successful_allocations: usize,
    #[cfg(test)]
    rejected_allocation: Option<(usize, usize)>,
}

const CERTIFICATE_BYTE_BUDGET: usize = 4 * 1024 * 1024 + 64 * 1024;

impl<const BYTE_BUDGET: usize> brotli_decompressor::Allocator<u8>
    for &mut BrotliByteAllocator<BYTE_BUDGET>
{
    type AllocatedMemory = BrotliBytes;

    fn alloc_cell(&mut self, length: usize) -> Self::AllocatedMemory {
        // Decompressor 6.1.0 checks u8 allocations before using them, including
        // a new ring allocated before the old one is freed during growth. Count
        // both live rings against the same budget. This bounds u8 working memory,
        // not the separate output buffer or grammar-bounded Huffman/u32 storage;
        // retain StandardAlloc for those other allocation types.
        if length > BYTE_BUDGET.saturating_sub(self.allocated) {
            #[cfg(test)]
            {
                self.rejected_allocation = Some((length, self.allocated));
            }
            return BrotliBytes::default();
        }
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(length).is_err() {
            return BrotliBytes::default();
        }
        bytes.resize(length, 0);
        self.allocated += length;
        #[cfg(test)]
        {
            self.peak_allocated = self.peak_allocated.max(self.allocated);
            self.successful_allocations += 1;
        }
        BrotliBytes(bytes)
    }

    fn free_cell(&mut self, memory: Self::AllocatedMemory) {
        self.allocated -= memory.0.len();
    }
}

impl CertificateCompressor for BrotliDecoder {
    const ALGORITHM: CertificateCompressionAlgorithm = CertificateCompressionAlgorithm::BROTLI;
    const CAN_COMPRESS: bool = false;
    const CAN_DECOMPRESS: bool = true;

    fn decompress<W: Write>(&self, input: &[u8], output: &mut W) -> std::io::Result<()> {
        decompress_certificate(
            input,
            output,
            &mut BrotliByteAllocator::<CERTIFICATE_BYTE_BUDGET>::default(),
        )
    }
}

fn decompress_certificate<W: Write, const BYTE_BUDGET: usize>(
    input: &[u8],
    output: &mut W,
    allocator: &mut BrotliByteAllocator<BYTE_BUDGET>,
) -> std::io::Result<()> {
    use brotli_decompressor::{BrotliDecompressStream, BrotliResult, BrotliState, StandardAlloc};

    const MAX_CERTIFICATE_BYTES: usize = 1024 * 1024;
    if input.len() > MAX_CERTIFICATE_BYTES {
        return Err(std::io::Error::other(
            "compressed certificate exceeds safety limit",
        ));
    }
    // Reject non-RFC large-window streams and bound actual ring allocation,
    // even before a peer certificate has passed its public-key pin check.
    let mut state = BrotliState::new_strict(
        allocator,
        StandardAlloc::default(),
        StandardAlloc::default(),
    );
    let mut decoded = vec![0; MAX_CERTIFICATE_BYTES + 1];
    let mut available_in = input.len();
    let mut input_offset = 0;
    let mut available_out = decoded.len();
    let mut output_offset = 0;
    let mut total_out = 0;
    let result = BrotliDecompressStream(
        &mut available_in,
        &mut input_offset,
        input,
        &mut available_out,
        &mut output_offset,
        &mut decoded,
        &mut total_out,
        &mut state,
    );
    if !matches!(result, BrotliResult::ResultSuccess)
        || input_offset != input.len()
        || output_offset > MAX_CERTIFICATE_BYTES
    {
        return Err(std::io::Error::other(
            "invalid or oversized compressed certificate",
        ));
    }
    output.write_all(&decoded[..output_offset])
}

#[cfg(test)]
mod tests;
