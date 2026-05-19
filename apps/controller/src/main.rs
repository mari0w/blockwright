use std::{
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::Path,
};

use blockwright_controller::{
    app, config, https as https_server, integrations::matrix, mcp, state::AppState,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const DEFAULT_HTTPS_OFFSET: u16 = 1;
const CA_CERT_DOWNLOAD_PATH: &str = "/web/blockwright-local-root-ca.cer";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenvy::dotenv().ok();
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "serve".to_string());
    init_tracing();

    let config = config::load()?;
    let state = AppState::new(config).await?;

    if mode == "mcp" {
        tracing::info!("blockwright MCP server starting on stdio");
        return mcp::serve_stdio(state).await;
    }

    if mode != "serve" {
        return Err(format!("unknown blockwright-controller mode: {mode}").into());
    }

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(state.config.server.port);
    let host = std::env::var("HOST").unwrap_or_else(|_| state.config.server.host.clone());
    let bind_addr = format!("{host}:{port}");

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    let actual_port = listener.local_addr()?.port();
    let lan_ip = primary_lan_ipv4();

    matrix::spawn_pollers(state.clone());
    maybe_spawn_https_server(&state, &host, actual_port, lan_ip).await;
    log_access_urls(&host, actual_port, lan_ip);
    axum::serve(listener, app::build_app(state)).await?;
    Ok(())
}

async fn maybe_spawn_https_server(
    state: &AppState,
    host: &str,
    http_port: u16,
    lan_ip: Option<Ipv4Addr>,
) {
    if env_flag_disabled("HTTPS_ENABLED") {
        print_access_line("Blockwright HTTPS 已禁用；手机语音需要 HTTPS 才能使用".to_string());
        return;
    }
    let https_port = std::env::var("HTTPS_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(http_port.saturating_add(DEFAULT_HTTPS_OFFSET));
    let bind_addr = format!("{host}:{https_port}");
    let assets = match https_server::ensure_assets(&state.config.storage.data_dir, lan_ip) {
        Ok(assets) => assets,
        Err(error) => {
            print_access_line(format!(
                "Blockwright HTTPS 证书生成失败：{error}；手机语音需要 HTTPS 才能使用"
            ));
            return;
        }
    };
    let config = match https_server::server_config(&assets) {
        Ok(config) => config,
        Err(error) => {
            print_access_line(format!(
                "Blockwright HTTPS 证书加载失败：{error}；手机语音需要 HTTPS 才能使用"
            ));
            return;
        }
    };
    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(listener) => listener,
        Err(error) => {
            print_access_line(format!(
                "Blockwright HTTPS 监听失败：{bind_addr}（{error}）；手机语音需要 HTTPS 才能使用"
            ));
            return;
        }
    };
    let actual_port = listener
        .local_addr()
        .map(|addr| addr.port())
        .unwrap_or(https_port);
    log_https_access_urls(host, actual_port, http_port, lan_ip, &assets.ca_cert_path);
    let https_state = state.clone();
    tokio::spawn(async move {
        let listener = https_server::TlsListener::new(listener, config);
        if let Err(error) = axum::serve(listener, app::build_app(https_state)).await {
            tracing::warn!("Blockwright HTTPS 服务退出：{error}");
        }
    });
}

fn log_access_urls(host: &str, port: u16, lan_ip: Option<Ipv4Addr>) {
    print_access_line(format!("Blockwright 本机访问：http://127.0.0.1:{port}/web"));
    match lan_ip {
        Some(ip) if lan_accessible_host(host) => {
            print_access_line(format!("Blockwright 局域网访问：http://{ip}:{port}/web"));
        }
        Some(ip) => {
            print_access_line(format!(
                "Blockwright 当前只监听 {host}:{port}，局域网地址 http://{ip}:{port}/web 可能无法访问；需要把 HOST 设为 0.0.0.0"
            ));
        }
        None => {
            print_access_line(
                "Blockwright 未检测到局域网 IPv4 地址，只展示本机访问地址".to_string(),
            );
        }
    }
}

fn log_https_access_urls(
    host: &str,
    https_port: u16,
    http_port: u16,
    lan_ip: Option<Ipv4Addr>,
    ca_cert_path: &Path,
) {
    print_access_line(format!(
        "Blockwright 本机 HTTPS：https://127.0.0.1:{https_port}/web"
    ));
    print_access_line(format!(
        "Blockwright 本机证书下载：{}",
        ca_certificate_download_url("127.0.0.1", http_port)
    ));
    match lan_ip {
        Some(ip) if lan_accessible_host(host) => {
            print_access_line(format!(
                "Blockwright 手机 HTTPS：https://{ip}:{https_port}/web"
            ));
            print_access_line(format!(
                "Blockwright 手机证书下载：{}",
                ca_certificate_download_url(&ip.to_string(), http_port)
            ));
        }
        Some(ip) => {
            print_access_line(format!(
                "Blockwright 当前只监听 {host}:{https_port}，局域网 HTTPS https://{ip}:{https_port}/web 可能无法访问；需要把 HOST 设为 0.0.0.0"
            ));
        }
        None => {
            print_access_line(
                "Blockwright 未检测到局域网 IPv4 地址，只展示本机 HTTPS 地址".to_string(),
            );
        }
    }
    print_access_line(format!(
        "Blockwright 自签根证书文件：{}；手机访问 HTTPS 前需要先安装并信任这个证书",
        ca_cert_path.display()
    ));
}

fn ca_certificate_download_url(host: &str, port: u16) -> String {
    format!("http://{host}:{port}{CA_CERT_DOWNLOAD_PATH}")
}

fn print_access_line(message: String) {
    eprintln!("{message}");
}

fn env_flag_disabled(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "0" | "false" | "no" | "off"
    )
}

fn primary_lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() => {
            Some(ip)
        }
        _ => None,
    }
}

fn lan_accessible_host(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if host == "0.0.0.0" || host == "::" {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => !ip.is_loopback() && !ip.is_unspecified(),
        Err(_) => false,
    }
}

fn init_tracing() {
    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
}

#[cfg(test)]
mod tests {
    use super::{ca_certificate_download_url, lan_accessible_host};

    #[test]
    fn wildcard_host_is_lan_accessible() {
        assert!(lan_accessible_host("0.0.0.0"));
        assert!(lan_accessible_host("::"));
        assert!(lan_accessible_host("[::]"));
    }

    #[test]
    fn loopback_host_is_not_lan_accessible() {
        assert!(!lan_accessible_host("127.0.0.1"));
        assert!(!lan_accessible_host("localhost"));
    }

    #[test]
    fn ca_certificate_download_url_uses_http_web_endpoint() {
        assert_eq!(
            ca_certificate_download_url("192.168.3.2", 8765),
            "http://192.168.3.2:8765/web/blockwright-local-root-ca.cer"
        );
    }
}
