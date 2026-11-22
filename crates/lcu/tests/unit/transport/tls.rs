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
