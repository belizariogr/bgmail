//! Brand logos (BIMI), checked the way Gmail and Apple Mail do before showing one.
//!
//! A logo is shown for a message only when all of this holds:
//! 1. The message passes DMARC through an aligned DKIM signature that we verify
//!    ourselves ([`BrandChecker::authenticated_domain`]). A provider's
//!    `Authentication-Results` header isn't trusted: where the provider adds
//!    none, the sender can write one.
//! 2. The domain's DMARC policy enforces (`quarantine` or `reject`, `pct=100`),
//!    as BIMI requires.
//! 3. The domain publishes a BIMI record (`default._bimi.<domain>`, else at its
//!    organizational domain) with a mark certificate (`a=`).
//! 4. That certificate chains to a mark verifying authority's root
//!    ([`roots`]), carries the BIMI key usage, is valid now and names the
//!    domain. The logo shown is the one inside it (RFC 3709 logotype), so the
//!    `l=` link is never fetched. Revocation isn't checked.
//!
//! DNS goes through `mail-auth`'s resolver (system configuration) on a
//! single-threaded Tokio runtime owned by the checker; the certificate is
//! fetched with the crate's blocking HTTP. Everything blocks: call it off the
//! UI thread.

use std::io::Read;
use std::time::{Duration, SystemTime};

use base64::Engine;
use mail_auth::dmarc::Policy;
use mail_auth::{AuthenticatedMessage, DmarcResult, MessageAuthenticator, SpfOutput};
use mail_core::{RemoteError, RemoteErrorKind};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName, TrustAnchor, UnixTime};

/// The extended key usage of a mark certificate (id-kp-BrandIndicatorforMessageIdentification,
/// 1.3.6.1.5.5.7.3.31), as the value bytes of its OID.
const BIMI_KEY_USAGE: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x1f];
/// id-pe-logotype (1.3.6.1.5.5.7.1.12), as the value bytes of its OID.
const LOGOTYPE_EXTENSION: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x01, 0x0c];
/// Largest certificate chain fetched.
const MAX_CERTIFICATE_BYTES: u64 = 512 * 1024;
/// Largest logo accepted, once decompressed.
const MAX_LOGO_BYTES: u64 = 1024 * 1024;
/// How deep a certificate's DER is walked looking for the logo.
const MAX_DER_DEPTH: usize = 16;
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Roots of the mark verifying authorities, as DER: those the BIMI Group lists
/// (<https://bimigroup.org/vmc/>), plus Entrust's, whose certificates already
/// issued are still valid. Each SHA-256 matches the CCADB record; Sectigo issues
/// under SSL.com's RSA root.
pub mod roots {
    /// DigiCert Verified Mark Root CA, from
    /// <https://cacerts.digicert.com/DigiCertVerifiedMarkRootCA.crt>.
    pub const DIGICERT: &[u8] = include_bytes!("bimi_roots/digicert_verified_mark_root_ca.der");
    /// GlobalSign Verified Mark Root R42, from
    /// <http://secure.globalsign.com/cacert/gsverifiedmarkrootr42.crt>.
    pub const GLOBALSIGN: &[u8] =
        include_bytes!("bimi_roots/globalsign_verified_mark_root_r42.der");
    /// SSL.com VMC RSA Root CA 2024, from
    /// <https://ssl.com/repo/certs/SSL.com-VMC-Root-2024-RSA.pem>.
    pub const SSL_COM_RSA: &[u8] = include_bytes!("bimi_roots/sslcom_vmc_rsa_root_ca_2024.der");
    /// SSL.com VMC ECC Root CA 2024, from
    /// <https://ssl.com/repo/certs/SSL.com-VMC-Root-2024-ECC.pem>.
    pub const SSL_COM_ECC: &[u8] = include_bytes!("bimi_roots/sslcom_vmc_ecc_root_ca_2024.der");
    /// Entrust Verified Mark Root Certification Authority - VMCR1, from the BIMI
    /// Group's copy <https://bimigroup.org/certs/entrust_vmc_specific.pem>
    /// (Entrust's own pages refuse automated downloads).
    pub const ENTRUST: &[u8] = include_bytes!("bimi_roots/entrust_verified_mark_root_vmcr1.der");

    /// Every root a mark certificate may chain to.
    pub const ALL: &[&[u8]] = &[DIGICERT, GLOBALSIGN, SSL_COM_RSA, SSL_COM_ECC, ENTRUST];
}

/// A domain's BIMI record (`v=BIMI1; l=…; a=…`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BimiRecord {
    /// `l=`: where the logo is. Empty when the domain declines to show one.
    pub logo: String,
    /// `a=`: the mark certificate (PEM chain), if any.
    pub authority: Option<String>,
}

/// Reads a BIMI TXT record. `None` unless it is a `v=BIMI1` record first.
pub fn parse_bimi_record(text: &str) -> Option<BimiRecord> {
    let mut tags = text
        .split(';')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(|tag| match tag.split_once('=') {
            Some((name, value)) => (name.trim(), value.trim()),
            None => (tag, ""),
        });
    let (name, version) = tags.next()?;
    if !name.eq_ignore_ascii_case("v") || version != "BIMI1" {
        return None;
    }
    let mut record = BimiRecord {
        logo: String::new(),
        authority: None,
    };
    for (name, value) in tags {
        match name.to_ascii_lowercase().as_str() {
            "l" => record.logo = value.to_string(),
            "a" if !value.is_empty() => record.authority = Some(value.to_string()),
            _ => {}
        }
    }
    Some(record)
}

/// Whether a DMARC policy is strict enough for BIMI: mail that fails is held or
/// refused, all of it.
pub fn policy_enforces(policy: Policy, percent: u8) -> bool {
    matches!(policy, Policy::Quarantine | Policy::Reject) && percent == 100
}

/// The organizational domain (`mail.nubank.com.br` → `nubank.com.br`), from the
/// public suffix list; the domain itself when it has none.
pub fn organizational_domain(domain: &str) -> &str {
    psl::domain_str(domain).unwrap_or(domain)
}

/// One DER element: its tag, its content and what follows it.
fn der_element(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    // High tag numbers don't occur in certificates.
    if tag & 0x1f == 0x1f {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (first as usize, rest)
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let length = rest[..count]
            .iter()
            .fold(0usize, |length, byte| (length << 8) | *byte as usize);
        (length, &rest[count..])
    };
    (rest.len() >= length).then(|| (tag, &rest[..length], &rest[length..]))
}

/// The elements in `input`, in order; stops at the first malformed one.
fn der_elements(mut input: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    std::iter::from_fn(move || {
        let (tag, content, rest) = der_element(input)?;
        input = rest;
        Some((tag, content))
    })
}

const TAG_OID: u8 = 0x06;
const TAG_OCTET_STRING: u8 = 0x04;
const TAG_IA5_STRING: u8 = 0x16;
const CONSTRUCTED: u8 = 0x20;

/// The value of the extension with OID `oid` in a certificate's DER.
fn extension_value<'a>(der: &'a [u8], oid: &[u8], depth: usize) -> Option<&'a [u8]> {
    if depth > MAX_DER_DEPTH {
        return None;
    }
    for (tag, content) in der_elements(der) {
        if tag & CONSTRUCTED == 0 {
            continue;
        }
        let mut children = der_elements(content);
        if let Some((TAG_OID, id)) = children.next() {
            if id == oid {
                // `critical` may come first; the value is the octet string.
                return children
                    .find(|(tag, _)| *tag == TAG_OCTET_STRING)
                    .map(|(_, value)| value);
            }
        }
        if let Some(value) = extension_value(content, oid, depth + 1) {
            return Some(value);
        }
    }
    None
}

/// The first IA5 string inside `der` that starts with `prefix`.
fn ia5_string_starting<'a>(der: &'a [u8], prefix: &[u8], depth: usize) -> Option<&'a [u8]> {
    if depth > MAX_DER_DEPTH {
        return None;
    }
    der_elements(der).find_map(|(tag, content)| {
        if tag == TAG_IA5_STRING {
            content.starts_with(prefix).then_some(content)
        } else if tag & CONSTRUCTED != 0 {
            ia5_string_starting(content, prefix, depth + 1)
        } else {
            None
        }
    })
}

/// The SVG of a `data:image/svg+xml;base64,…` URI, gunzipped when compressed (as
/// logotypes are), up to [`MAX_LOGO_BYTES`].
pub fn svg_from_data_uri(uri: &[u8]) -> Option<Vec<u8>> {
    let uri = std::str::from_utf8(uri).ok()?;
    let (meta, data) = uri.strip_prefix("data:")?.split_once(',')?;
    let mut meta = meta.split(';').map(str::trim);
    if !meta.next()?.eq_ignore_ascii_case("image/svg+xml") || !meta.any(|m| m == "base64") {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .ok()?;
    let svg = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take(MAX_LOGO_BYTES + 1)
            .read_to_end(&mut out)
            .ok()?;
        out
    } else {
        bytes
    };
    (!svg.is_empty() && svg.len() as u64 <= MAX_LOGO_BYTES).then_some(svg)
}

/// The logo inside a mark certificate (its RFC 3709 logotype extension).
pub fn certificate_logo(certificate_der: &[u8]) -> Option<Vec<u8>> {
    let extension = extension_value(certificate_der, LOGOTYPE_EXTENSION, 0)?;
    let uri = ia5_string_starting(extension, b"data:", 0)?;
    svg_from_data_uri(uri)
}

/// Why a mark certificate was refused.
#[derive(Debug)]
pub enum MarkError {
    /// No certificate in the PEM.
    Empty,
    /// Doesn't chain to a known root, lacks the BIMI usage, or isn't valid now.
    Untrusted(String),
    /// Names none of the domains.
    WrongDomain,
    /// Carries no SVG logo.
    NoLogo,
}

/// Checks a mark certificate chain (PEM, leaf first) against `roots` at `now`
/// for one of `domains`, and returns the logo it carries.
pub fn verify_mark_certificate(
    pem: &[u8],
    domains: &[&str],
    roots: &[&[u8]],
    now: SystemTime,
) -> Result<Vec<u8>, MarkError> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(pem)
        .filter_map(Result::ok)
        .collect();
    let (leaf, intermediates) = chain.split_first().ok_or(MarkError::Empty)?;
    let anchors: Vec<TrustAnchor<'_>> = roots
        .iter()
        .filter_map(|der| {
            webpki::anchor_from_trusted_cert(&CertificateDer::from(*der))
                .ok()
                .map(|anchor| anchor.to_owned())
        })
        .collect();
    let certificate = webpki::EndEntityCert::try_from(leaf)
        .map_err(|error| MarkError::Untrusted(error.to_string()))?;
    let now = UnixTime::since_unix_epoch(
        now.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default(),
    );
    certificate
        .verify_for_usage(
            webpki::ALL_VERIFICATION_ALGS,
            &anchors,
            intermediates,
            now,
            webpki::KeyUsage::required(BIMI_KEY_USAGE),
            None,
            None,
        )
        .map_err(|error| MarkError::Untrusted(error.to_string()))?;
    let named = domains.iter().any(|domain| {
        ServerName::try_from(*domain)
            .is_ok_and(|name| certificate.verify_is_valid_for_subject_name(&name).is_ok())
    });
    if !named {
        return Err(MarkError::WrongDomain);
    }
    certificate_logo(leaf).ok_or(MarkError::NoLogo)
}

/// Runs the checks that need the network. One per worker thread: it owns a
/// small Tokio runtime for the resolver.
pub struct BrandChecker {
    runtime: tokio::runtime::Runtime,
    resolver: MessageAuthenticator,
}

impl BrandChecker {
    /// A checker on the system's DNS configuration.
    pub fn new() -> Result<Self, RemoteError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| RemoteError::new(RemoteErrorKind::Other, error.to_string()))?;
        // Hickory's resolver must be built inside the runtime it runs on.
        let resolver = runtime
            .block_on(async { MessageAuthenticator::new_system_conf() })
            .map_err(|error| RemoteError::new(RemoteErrorKind::Network, error.to_string()))?;
        Ok(Self { runtime, resolver })
    }

    /// The `From` domain of `raw` when the message passes DMARC through a DKIM
    /// signature aligned with it, under a policy that enforces. SPF isn't
    /// checked: the connecting address isn't known to a client.
    pub fn authenticated_domain(&self, raw: &[u8]) -> Option<String> {
        let message = AuthenticatedMessage::parse(raw)?;
        self.runtime.block_on(async {
            let dkim = self.resolver.verify_dkim(&message).await;
            let spf = SpfOutput::new(String::new());
            let parameters =
                mail_auth::dmarc::verify::DmarcParameters::new(&message, &dkim, "", &spf)
                    .with_domain_suffix_fn(organizational_domain);
            let dmarc = self.resolver.verify_dmarc(parameters).await;
            let percent = dmarc.dmarc_record().map_or(0, |record| record.pct);
            (*dmarc.dkim_result() == DmarcResult::Pass && policy_enforces(dmarc.policy(), percent))
                .then(|| dmarc.domain().to_ascii_lowercase())
        })
    }

    /// The BIMI record of `domain`, else of its organizational domain.
    fn bimi_record(&self, domain: &str) -> Option<BimiRecord> {
        let organizational = organizational_domain(domain);
        let mut candidates = vec![domain];
        if organizational != domain {
            candidates.push(organizational);
        }
        candidates.into_iter().find_map(|candidate| {
            let text = self
                .runtime
                .block_on(
                    self.resolver
                        .txt_raw_lookup(format!("default._bimi.{candidate}.")),
                )
                .ok()?;
            parse_bimi_record(&String::from_utf8_lossy(&text))
        })
    }

    /// The verified logo of an authenticated `domain`: `Ok(None)` when it has
    /// none we can show (no record, declined, no or bad certificate), `Err` when
    /// the certificate couldn't be fetched, which may pass.
    pub fn logo(&self, domain: &str, now: SystemTime) -> Result<Option<Vec<u8>>, RemoteError> {
        let Some(record) = self.bimi_record(domain) else {
            return Ok(None);
        };
        let Some(authority) = record.authority.filter(|_| !record.logo.is_empty()) else {
            return Ok(None);
        };
        if !authority.starts_with("https://") {
            return Ok(None);
        }
        let Some(pem) = crate::http::get_bytes(&authority, HTTP_TIMEOUT, MAX_CERTIFICATE_BYTES)?
        else {
            return Ok(None);
        };
        let domains = [domain, organizational_domain(domain)];
        Ok(verify_mark_certificate(&pem, &domains, roots::ALL, now).ok())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// Nubank's mark certificate chain (leaf, DigiCert intermediate, root), as
    /// published at its BIMI record's `a=`. Valid 2026-01-22 to 2027-01-21.
    const NUBANK_PEM: &[u8] = include_bytes!("bimi_roots/test_nubank_vmc.pem");

    fn at(year_days: u64) -> SystemTime {
        // Days since 2026-01-01.
        SystemTime::UNIX_EPOCH + Duration::from_secs((20454 + year_days) * 86_400)
    }

    /// The embedded roots are the ones checked against CCADB, and each parses
    /// as a trust anchor.
    #[test]
    fn roots_are_the_published_ones() {
        use sha2::Digest;
        let expected = [
            (
                roots::DIGICERT,
                "504386c9ee8932fecc95fade427f69c3e2534b7310489e300fee448e33c46b42",
            ),
            (
                roots::GLOBALSIGN,
                "cd122cb877c6928b9017b0f0b80dbd508196300bbd03cd7356c3beef524e7e0b",
            ),
            (
                roots::SSL_COM_RSA,
                "8f9d1b7698886782a599b48510651c66a1aa0c5ca3192097bdc68534154bd30d",
            ),
            (
                roots::SSL_COM_ECC,
                "1b82e7f4910b51e3e802a493acdc17ff58eac8b9eb7c09b52ac6cd2efb83598c",
            ),
            (
                roots::ENTRUST,
                "7831d95a47d42508cd5c9e6264f9096bac19f04eb9b7c8bdd35fffc71c189617",
            ),
        ];
        assert_eq!(expected.len(), roots::ALL.len());
        for (der, fingerprint) in expected {
            let digest = sha2::Sha256::digest(der);
            let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(hex, fingerprint);
            assert!(webpki::anchor_from_trusted_cert(&CertificateDer::from(der)).is_ok());
        }
    }

    #[test]
    fn reads_a_bimi_record() {
        assert_eq!(
            parse_bimi_record("v=BIMI1;l=https://x/l.svg;a=https://x/a.pem;avp=personal"),
            Some(BimiRecord {
                logo: "https://x/l.svg".into(),
                authority: Some("https://x/a.pem".into()),
            })
        );
        assert_eq!(
            parse_bimi_record(" v=BIMI1; l=; a=").unwrap(),
            BimiRecord {
                logo: String::new(),
                authority: None
            }
        );
        // The version comes first, and only BIMI1.
        assert_eq!(parse_bimi_record("l=x; v=BIMI1"), None);
        assert_eq!(parse_bimi_record("v=BIMI2; l=x"), None);
        assert_eq!(parse_bimi_record("v=spf1 -all"), None);
    }

    #[test]
    fn only_an_enforcing_policy_on_all_mail_counts() {
        assert!(policy_enforces(Policy::Reject, 100));
        assert!(policy_enforces(Policy::Quarantine, 100));
        assert!(!policy_enforces(Policy::Quarantine, 50));
        assert!(!policy_enforces(Policy::None, 100));
        assert!(!policy_enforces(Policy::Unspecified, 100));
    }

    #[test]
    fn finds_the_organizational_domain() {
        assert_eq!(organizational_domain("mail.nubank.com.br"), "nubank.com.br");
        assert_eq!(organizational_domain("nubank.com.br"), "nubank.com.br");
        assert_eq!(organizational_domain("a.b.example.co.uk"), "example.co.uk");
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn decodes_logo_data_uris() {
        let svg = b"<svg/>";
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        let plain = format!("data:image/svg+xml;base64,{}", b64(svg));
        assert_eq!(svg_from_data_uri(plain.as_bytes()).unwrap(), svg);
        let zipped = format!("data:image/svg+xml;base64,{}", b64(&gzip(svg)));
        assert_eq!(svg_from_data_uri(zipped.as_bytes()).unwrap(), svg);
        // Another type, not base64, or bigger than allowed once unzipped.
        let png = format!("data:image/png;base64,{}", b64(svg));
        assert_eq!(svg_from_data_uri(png.as_bytes()), None);
        assert_eq!(svg_from_data_uri(b"data:image/svg+xml,<svg/>"), None);
        let bomb = vec![b' '; MAX_LOGO_BYTES as usize + 1];
        let bomb = format!("data:image/svg+xml;base64,{}", b64(&gzip(&bomb)));
        assert_eq!(svg_from_data_uri(bomb.as_bytes()), None);
    }

    #[test]
    fn malformed_der_ends_the_walk() {
        assert_eq!(der_element(&[]), None);
        // Length past the end, and a length of more than four bytes.
        assert_eq!(der_element(&[0x30, 0x05, 0x01]), None);
        assert_eq!(der_element(&[0x30, 0x85, 1, 1, 1, 1, 1]), None);
        assert_eq!(
            der_element(&[0x04, 0x81, 0x01, 0xaa]),
            Some((0x04, &[0xaa][..], &[][..]))
        );
        assert_eq!(certificate_logo(&[0x30, 0x03, 0x06, 0x01]), None);
    }

    #[test]
    fn verifies_a_real_mark_certificate_and_reads_its_logo() {
        let svg = verify_mark_certificate(NUBANK_PEM, &["nubank.com.br"], roots::ALL, at(150))
            .expect("Nubank's VMC verifies");
        let text = String::from_utf8_lossy(&svg);
        assert!(text.contains("<svg"), "{text:.200}");
        // Any domain the certificate names.
        assert!(verify_mark_certificate(NUBANK_PEM, &["nu.com.br"], roots::ALL, at(150)).is_ok());
    }

    #[test]
    fn refuses_a_mark_certificate_for_another_domain_time_or_root() {
        let wrong_domain =
            verify_mark_certificate(NUBANK_PEM, &["nubamk.com.br"], roots::ALL, at(150));
        assert!(matches!(wrong_domain, Err(MarkError::WrongDomain)));
        // Before it was issued, and after it expired.
        for when in [at(0), at(400)] {
            let refused = verify_mark_certificate(NUBANK_PEM, &["nubank.com.br"], roots::ALL, when);
            assert!(matches!(refused, Err(MarkError::Untrusted(_))));
        }
        // Without its authority's root, the chain's own root counts for nothing.
        let unknown = verify_mark_certificate(NUBANK_PEM, &["nubank.com.br"], &[], at(150));
        assert!(matches!(unknown, Err(MarkError::Untrusted(_))));
        assert!(matches!(
            verify_mark_certificate(b"", &["nubank.com.br"], roots::ALL, at(150)),
            Err(MarkError::Empty)
        ));
    }

    /// The authority's own certificate can't stand in for a brand's mark.
    #[test]
    fn an_authority_certificate_is_no_mark() {
        // The DigiCert intermediate alone, presented as the leaf.
        let intermediate = CertificateDer::pem_slice_iter(NUBANK_PEM)
            .nth(1)
            .unwrap()
            .unwrap();
        let pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            base64::engine::general_purpose::STANDARD.encode(intermediate.as_ref())
        );
        let refused =
            verify_mark_certificate(pem.as_bytes(), &["nubank.com.br"], roots::ALL, at(150));
        assert!(refused.is_err());
    }

    /// Against the real DNS and DigiCert: Nubank's published logo. Needs the network.
    #[test]
    #[ignore = "network"]
    fn live_nubank_logo() {
        let checker = BrandChecker::new().unwrap();
        let logo = checker.logo("nubank.com.br", SystemTime::now()).unwrap();
        assert!(logo.is_some_and(|svg| String::from_utf8_lossy(&svg).contains("<svg")));
        // `BIMI_RAW`: a message whose sender publishes BIMI, checked end to end.
        if let Ok(path) = std::env::var("BIMI_RAW") {
            let raw = std::fs::read(path).unwrap();
            let domain = checker.authenticated_domain(&raw).expect("passes DMARC");
            let logo = checker.logo(&domain, SystemTime::now()).unwrap();
            eprintln!("{domain}: logo {:?} bytes", logo.map(|svg| svg.len()));
        }
    }
}
