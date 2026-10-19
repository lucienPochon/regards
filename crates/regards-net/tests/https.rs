//! The HTTPS client against a local TLS server with a throwaway CA: what it
//! sends, and a chunked answer read back.

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
};
use regards_net::TlsOptions;
use regards_net::https::post_form;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

fn server_config() -> (String, Arc<ServerConfig>) {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::DigitalSignature];
    ca.distinguished_name.push(DnType::CommonName, "Regards Test CA");
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::from_params(&ca, &ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let mut leaf = CertificateParams::new(vec!["localhost".to_owned()]).unwrap();
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let leaf_cert = leaf.signed_by(&leaf_key, &issuer).unwrap();
    let config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![leaf_cert.der().clone()],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        )
        .unwrap();
    (ca_cert.pem(), Arc::new(config))
}

#[tokio::test]
async fn a_form_goes_out_and_a_chunked_answer_comes_back() {
    let (ca, config) = server_config();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = TlsAcceptor::from(config).accept(tcp).await.unwrap();
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        // Headers, then the body its Content-Length gives.
        loop {
            let read = tls.read(&mut chunk).await.unwrap();
            request.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&request);
            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if body.len() >= length {
                    break;
                }
            }
        }
        tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await
            .unwrap();
        tls.write_all(b"5\r\n{\"ok\"\r\n5\r\n:true\r\n1\r\n}\r\n0\r\n\r\n")
            .await
            .unwrap();
        tls.shutdown().await.unwrap();
        String::from_utf8(request).unwrap()
    });

    let options = TlsOptions::platform().with_extra_roots_pem(ca.as_bytes()).unwrap();
    let url = format!("https://localhost:{port}/oauth2/token");
    let answer = post_form(
        &url,
        &[("grant_type", "authorization_code"), ("code", "a/b c")],
        &options,
    )
    .await
    .unwrap();
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, b"{\"ok\":true}");

    let request = server.await.unwrap();
    assert!(request.starts_with("POST /oauth2/token HTTP/1.1\r\n"), "{request}");
    assert!(request.contains(&format!("Host: localhost:{port}\r\n")));
    assert!(request.contains("Content-Type: application/x-www-form-urlencoded\r\n"));
    assert!(request.ends_with("\r\n\r\ngrant_type=authorization_code&code=a%2Fb%20c"));
}

#[tokio::test]
async fn an_unknown_certificate_is_refused() {
    let (_ca, config) = server_config();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let _ = TlsAcceptor::from(config).accept(tcp).await;
    });
    let refused = post_form(
        &format!("https://localhost:{port}/token"),
        &[("a", "b")],
        &TlsOptions::platform(),
    )
    .await;
    assert!(refused.is_err());
}
