use std::{
    fs, io,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use axum::serve::Listener;
use base64::{engine::general_purpose, Engine as _};
use rustls::ServerConfig;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{
    net::{TcpListener, TcpStream},
    time::sleep,
};
use tokio_rustls::{server::TlsStream, TlsAcceptor};

pub struct HttpsAssets {
    pub ca_cert_path: PathBuf,
    pub server_cert_path: PathBuf,
    pub server_key_path: PathBuf,
}

pub struct TlsListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl TlsListener {
    pub fn new(listener: TcpListener, config: Arc<ServerConfig>) -> Self {
        Self {
            listener,
            acceptor: TlsAcceptor::from(config),
        }
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (stream, addr) = match self.listener.accept().await {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!("HTTPS accept failed: {error}");
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            match self.acceptor.accept(stream).await {
                Ok(stream) => return (stream, addr),
                Err(error) => {
                    if is_untrusted_certificate_alert(&error.to_string()) {
                        tracing::debug!(
                            %addr,
                            error = %error,
                            "HTTPS client has not trusted the Blockwright certificate yet"
                        );
                    } else {
                        tracing::warn!(%addr, "HTTPS TLS handshake failed: {error}");
                    }
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

pub fn ca_certificate_path(data_dir: &Path) -> PathBuf {
    cert_dir(data_dir).join("blockwright-local-ca.crt")
}

pub fn certificate_der_from_pem(
    source: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    pem_section(source, "CERTIFICATE")
}

pub fn ensure_assets(
    data_dir: &Path,
    lan_ip: Option<Ipv4Addr>,
) -> Result<HttpsAssets, Box<dyn std::error::Error + Send + Sync>> {
    let dir = cert_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let ca_key_path = dir.join("blockwright-local-ca.key");
    let ca_cert_path = dir.join("blockwright-local-ca.crt");
    let server_key_path = dir.join("blockwright-server.key");
    let server_cert_path = dir.join("blockwright-server.crt");
    let csr_path = dir.join("blockwright-server.csr");
    let config_path = dir.join("blockwright-openssl.cnf");

    if !ca_key_path.exists() || !ca_cert_path.exists() {
        run_openssl([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-sha256",
            "-days",
            "3650",
            "-nodes",
            "-subj",
            "/CN=Blockwright Local Root CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-keyout",
            path_str(&ca_key_path)?,
            "-out",
            path_str(&ca_cert_path)?,
        ])?;
    }

    fs::write(&config_path, openssl_config(lan_ip))?;
    run_openssl([
        "req",
        "-new",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        path_str(&server_key_path)?,
        "-out",
        path_str(&csr_path)?,
        "-config",
        path_str(&config_path)?,
    ])?;
    run_openssl([
        "x509",
        "-req",
        "-in",
        path_str(&csr_path)?,
        "-CA",
        path_str(&ca_cert_path)?,
        "-CAkey",
        path_str(&ca_key_path)?,
        "-CAcreateserial",
        "-out",
        path_str(&server_cert_path)?,
        "-days",
        "825",
        "-sha256",
        "-extfile",
        path_str(&config_path)?,
        "-extensions",
        "v3_req",
    ])?;

    Ok(HttpsAssets {
        ca_cert_path,
        server_cert_path,
        server_key_path,
    })
}

pub fn server_config(
    assets: &HttpsAssets,
) -> Result<Arc<ServerConfig>, Box<dyn std::error::Error + Send + Sync>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cert_der = pem_section(
        &fs::read_to_string(&assets.server_cert_path)?,
        "CERTIFICATE",
    )?;
    let key_der = pem_section(&fs::read_to_string(&assets.server_key_path)?, "PRIVATE KEY")?;
    let certs = vec![CertificateDer::from(cert_der)];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(Arc::new(config))
}

fn cert_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("https")
}

fn openssl_config(lan_ip: Option<Ipv4Addr>) -> String {
    let mut alt_names = vec![
        "DNS.1 = localhost".to_string(),
        "IP.1 = 127.0.0.1".to_string(),
    ];
    if let Some(ip) = lan_ip {
        alt_names.push(format!("IP.2 = {ip}"));
    }
    format!(
        r#"[req]
distinguished_name = req_distinguished_name
req_extensions = v3_req
prompt = no

[req_distinguished_name]
CN = Blockwright Local HTTPS

[v3_req]
basicConstraints = CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
{}
"#,
        alt_names.join("\n")
    )
}

fn run_openssl<const N: usize>(
    args: [&str; N],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let output = Command::new("openssl").args(args).output()?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("openssl 生成 HTTPS 证书失败：{}", stderr.trim()).into())
}

fn path_str(path: &Path) -> Result<&str, Box<dyn std::error::Error + Send + Sync>> {
    path.to_str()
        .ok_or_else(|| format!("路径不是有效 UTF-8：{}", path.display()).into())
}

fn is_untrusted_certificate_alert(message: &str) -> bool {
    let normalized = message
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '_' && *character != '-')
        .collect::<String>()
        .to_ascii_lowercase();
    normalized.contains("certificateunknown")
        || normalized.contains("unknownissuer")
        || normalized.contains("unknownca")
}

fn pem_section(
    source: &str,
    label: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let body = source
        .split(&begin)
        .nth(1)
        .and_then(|value| value.split(&end).next())
        .ok_or_else(|| format!("证书文件缺少 {label} PEM 段"))?;
    let compact = body.lines().map(str::trim).collect::<String>();
    Ok(general_purpose::STANDARD.decode(compact)?)
}

#[cfg(test)]
mod tests {
    use super::{is_untrusted_certificate_alert, openssl_config};
    use std::net::Ipv4Addr;

    #[test]
    fn openssl_config_includes_localhost_and_lan_ip() {
        let source = openssl_config(Some(Ipv4Addr::new(192, 168, 3, 2)));

        assert!(source.contains("DNS.1 = localhost"));
        assert!(source.contains("IP.1 = 127.0.0.1"));
        assert!(source.contains("IP.2 = 192.168.3.2"));
        assert!(source.contains("extendedKeyUsage = serverAuth"));
    }

    #[test]
    fn untrusted_certificate_alert_matches_mobile_tls_failure() {
        assert!(is_untrusted_certificate_alert(
            "received fatal alert: CertificateUnknown"
        ));
        assert!(is_untrusted_certificate_alert(
            "invalid peer certificate: UnknownIssuer"
        ));
        assert!(!is_untrusted_certificate_alert("connection reset by peer"));
    }
}
