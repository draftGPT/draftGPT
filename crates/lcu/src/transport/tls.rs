//! TLS for the LCU's local HTTPS/WSS server.
//!
//! The client presents a certificate issued by Riot's own root CA ("LoL Game
//! Engineering Certificate Authority", `certs/riotgames.pem`). That root is an
//! old X.509 v1 certificate with SHA-1 signatures, which webpki (rustls' default
//! verifier) refuses, and the leaf is not issued for `127.0.0.1`. So we verify
//! the chain ourselves:
//!
//! - the leaf (or one intermediate) must be signed by the Riot root's key,
//! - the TLS handshake signature must verify against the leaf's public key,
//! - the hostname is not checked (we only ever connect to 127.0.0.1),
//! - validity dates are not checked (the root is pinned, the host is loopback).
//!
//! Setting `DRAFTGPT_LCU_INSECURE_TLS=1` skips certificate checks entirely, as a
//! last resort if a client update breaks this. It is logged loudly.

use std::sync::{Arc, LazyLock};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, DistinguishedName, Error as TlsError, SignatureScheme};
use x509_parser::prelude::*;

use super::TransportError;

const RIOT_ROOT_PEM: &[u8] = include_bytes!("../../certs/riotgames.pem");
pub const INSECURE_ENV: &str = "DRAFTGPT_LCU_INSECURE_TLS";

/// Installs ring as the process-wide rustls provider (no-op if one is set).
///
/// `lcu` builds reqwest without a bundled provider to keep the Windows
/// cross-compile free of aws-lc. Because Cargo unifies features, every reqwest
/// client in the same binary then needs a process default, or it panics when
/// built. Binaries call this at startup; `client_config` calls it too.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// rustls client config for talking to the LCU, built once per process.
/// `alpn` is set for HTTP/1.1 (the LCU's WebSocket upgrade also runs over
/// HTTP/1.1).
pub fn client_config() -> Result<Arc<rustls::ClientConfig>, TransportError> {
    static CONFIG: LazyLock<Result<Arc<rustls::ClientConfig>, String>> =
        LazyLock::new(|| build_client_config().map_err(|e| e.to_string()));
    CONFIG.clone().map_err(TransportError::Tls)
}

fn build_client_config() -> Result<Arc<rustls::ClientConfig>, TransportError> {
    install_crypto_provider();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier: Arc<dyn ServerCertVerifier> = if insecure_requested() {
        tracing::warn!(
            "{INSECURE_ENV} is set: LCU certificate is NOT verified (loopback only, debugging use)"
        );
        Arc::new(AcceptAnyCert {
            provider: provider.clone(),
        })
    } else {
        Arc::new(RiotVerifier::new(provider.clone())?)
    };
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| TransportError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

fn insecure_requested() -> bool {
    std::env::var(INSECURE_ENV).is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn root_der() -> Result<Vec<u8>, TransportError> {
    CertificateDer::from_pem_slice(RIOT_ROOT_PEM)
        .map(|c| c.to_vec())
        .map_err(|e| TransportError::Tls(format!("riotgames.pem: {e}")))
}

#[derive(Debug)]
struct RiotVerifier {
    root_der: Vec<u8>,
    provider: Arc<CryptoProvider>,
}

impl RiotVerifier {
    fn new(provider: Arc<CryptoProvider>) -> Result<Self, TransportError> {
        Self::with_root(root_der()?, provider)
    }

    /// Verifier trusting a given root. Tests use it to run a real handshake.
    fn with_root(root_der: Vec<u8>, provider: Arc<CryptoProvider>) -> Result<Self, TransportError> {
        // Fail early if the root doesn't parse.
        X509Certificate::from_der(&root_der)
            .map_err(|e| TransportError::Tls(format!("bad root certificate: {e}")))?;
        Ok(Self { root_der, provider })
    }

    /// True if `cert` (DER) is signed by the key in `issuer` (DER).
    fn signed_by(cert: &[u8], issuer: &[u8]) -> bool {
        let (Ok((_, cert)), Ok((_, issuer))) = (
            X509Certificate::from_der(cert),
            X509Certificate::from_der(issuer),
        ) else {
            return false;
        };
        cert.verify_signature(Some(issuer.public_key())).is_ok()
    }

    /// Leaf signed by the root, or by an intermediate that the root signed.
    fn chain_ok(&self, end_entity: &[u8], intermediates: &[&[u8]]) -> bool {
        if Self::signed_by(end_entity, &self.root_der) {
            return true;
        }
        intermediates.iter().any(|inter| {
            Self::signed_by(inter, &self.root_der) && Self::signed_by(end_entity, inter)
        })
    }
}

impl ServerCertVerifier for RiotVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let inters: Vec<&[u8]> = intermediates.iter().map(AsRef::as_ref).collect();
        if self.chain_ok(end_entity.as_ref(), &inters) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::General(
                "LCU certificate is not signed by the Riot Games root CA".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_handshake_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_handshake_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        supported_schemes(&self.provider)
    }

    fn root_hint_subjects(&self) -> Option<&[DistinguishedName]> {
        None
    }
}

fn supported_schemes(provider: &CryptoProvider) -> Vec<SignatureScheme> {
    provider
        .signature_verification_algorithms
        .supported_schemes()
        .into_iter()
        .filter(|s| ring_algorithm(*s).is_some())
        .collect()
}

/// Verifies the handshake signature with ring directly, using the public key
/// parsed by x509-parser. (rustls' helper would parse the leaf with webpki, which
/// may reject Riot's certificates for the same reasons as the chain.)
fn verify_handshake_signature(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
) -> Result<HandshakeSignatureValid, TlsError> {
    let (_, parsed) = X509Certificate::from_der(cert.as_ref())
        .map_err(|e| TlsError::General(format!("unparsable LCU certificate: {e}")))?;
    let alg = ring_algorithm(dss.scheme).ok_or(TlsError::PeerIncompatible(
        rustls::PeerIncompatible::NoSignatureSchemesInCommon,
    ))?;
    let key = parsed.public_key().subject_public_key.data.as_ref();
    ring::signature::UnparsedPublicKey::new(alg, key)
        .verify(message, dss.signature())
        .map(|()| HandshakeSignatureValid::assertion())
        .map_err(|_| TlsError::General("LCU handshake signature invalid".into()))
}

fn ring_algorithm(
    scheme: SignatureScheme,
) -> Option<&'static dyn ring::signature::VerificationAlgorithm> {
    use ring::signature as s;
    Some(match scheme {
        SignatureScheme::RSA_PKCS1_SHA256 => &s::RSA_PKCS1_2048_8192_SHA256,
        SignatureScheme::RSA_PKCS1_SHA384 => &s::RSA_PKCS1_2048_8192_SHA384,
        SignatureScheme::RSA_PKCS1_SHA512 => &s::RSA_PKCS1_2048_8192_SHA512,
        SignatureScheme::RSA_PSS_SHA256 => &s::RSA_PSS_2048_8192_SHA256,
        SignatureScheme::RSA_PSS_SHA384 => &s::RSA_PSS_2048_8192_SHA384,
        SignatureScheme::RSA_PSS_SHA512 => &s::RSA_PSS_2048_8192_SHA512,
        SignatureScheme::ECDSA_NISTP256_SHA256 => &s::ECDSA_P256_SHA256_ASN1,
        SignatureScheme::ECDSA_NISTP384_SHA384 => &s::ECDSA_P384_SHA384_ASN1,
        SignatureScheme::ED25519 => &s::ED25519,
        _ => return None,
    })
}

/// Only used when `DRAFTGPT_LCU_INSECURE_TLS=1`.
#[derive(Debug)]
struct AcceptAnyCert {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_handshake_signature(m, c, d)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_handshake_signature(m, c, d)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        supported_schemes(&self.provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verifier() -> RiotVerifier {
        RiotVerifier::new(Arc::new(rustls::crypto::ring::default_provider())).unwrap()
    }

    #[test]
    fn embedded_root_is_riot_ca() {
        let der = root_der().unwrap();
        let (_, cert) = X509Certificate::from_der(&der).unwrap();
        let cn = cert.subject().iter_common_name().next().unwrap();
        assert_eq!(
            cn.as_str().unwrap(),
            "LoL Game Engineering Certificate Authority"
        );
    }

    #[test]
    fn root_verifies_its_own_sha1_signature() {
        // The root is self-signed with sha1WithRSA; this proves x509-parser can
        // verify the legacy algorithm the LCU chain uses.
        let der = root_der().unwrap();
        assert!(RiotVerifier::signed_by(&der, &der));
    }

    #[test]
    fn rejects_cert_not_from_riot() {
        let other = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let der = other.cert.der().to_vec();
        assert!(!verifier().chain_ok(&der, &[]));
        // Also when the foreign cert is offered as an intermediate.
        assert!(!verifier().chain_ok(&der, &[&der]));
    }

    #[test]
    fn config_builds() {
        client_config().unwrap();
    }

    struct TestChain {
        ca: Vec<u8>,
        leaf: rustls::pki_types::CertificateDer<'static>,
        key: rustls::pki_types::PrivateKeyDer<'static>,
    }

    /// A CA and a leaf issued for "rclient" (not 127.0.0.1), like the LCU's.
    fn test_chain() -> TestChain {
        use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issuer = Issuer::new(ca_params, ca_key);
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec!["rclient".to_string()])
            .unwrap()
            .signed_by(&leaf_key, &issuer)
            .unwrap();
        TestChain {
            ca: ca.der().to_vec(),
            leaf: leaf.der().clone(),
            key: rustls::pki_types::PrivateKeyDer::try_from(leaf_key.serialize_der()).unwrap(),
        }
    }

    /// Runs a TLS server presenting `chain` and connects to it with `verifier`.
    async fn handshake(chain: TestChain, verifier: RiotVerifier) -> std::io::Result<String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server_cfg = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![chain.leaf], chain.key)
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_cfg));
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            if let Ok(mut tls) = acceptor.accept(tcp).await {
                let _ = tls.write_all(b"ok").await;
                let _ = tls.shutdown().await;
            }
        });
        let client_cfg = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(client_cfg));
        let tcp = tokio::net::TcpStream::connect(addr).await?;
        let mut tls = connector
            .connect(ServerName::IpAddress(addr.ip().into()), tcp)
            .await?;
        let mut buf = String::new();
        tls.read_to_string(&mut buf).await?;
        Ok(buf)
    }

    #[tokio::test]
    async fn handshake_succeeds_with_trusted_root_despite_hostname() {
        let chain = test_chain();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = RiotVerifier::with_root(chain.ca.clone(), provider).unwrap();
        assert_eq!(handshake(chain, verifier).await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn handshake_fails_when_root_differs() {
        assert!(handshake(test_chain(), verifier()).await.is_err());
    }
}
