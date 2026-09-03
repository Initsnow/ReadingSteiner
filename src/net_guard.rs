//! 出站请求的 SSRF 防护。
//!
//! 抓取用户可控 URL（预览标题、通知图片下载）前统一校验：仅允许 http/https，
//! 且目标不得解析到私网 / 环回 / 链路本地等内网地址，避免把 daemon 当作内网探针。

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

use crate::error::{Error, Result};

/// 校验出站 http/https URL 的安全性。合法时返回解析后的 URL。
pub fn assert_public_http_url(url: &str) -> Result<url::Url> {
    let parsed = url::Url::parse(url).map_err(|_| Error::config(format!("invalid url: {url}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::config(format!(
            "unsupported url scheme: {}",
            parsed.scheme()
        )));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::config(format!("url missing host: {url}")))?;

    // host 为 IP 字面量时直接校验；否则尽力解析域名后校验首个地址。
    let ip = match parsed.host() {
        Some(url::Host::Ipv4(v)) => Some(IpAddr::V4(v)),
        Some(url::Host::Ipv6(v)) => Some(IpAddr::V6(v)),
        _ => (host, parsed.port_or_known_default().unwrap_or(80))
            .to_socket_addrs()
            .ok()
            .and_then(|mut it| it.next())
            .map(|s: SocketAddr| s.ip()),
    };

    if let Some(ip) = ip
        && is_private_ip(ip)
    {
        return Err(Error::config(format!(
            "url target resolves to private/internal address: {ip}"
        )));
    }
    Ok(parsed)
}

/// 判断 IP 是否属于私网 / 环回 / 链路本地 / 未指定等内网地址段。
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                || is_reserved_v4(v4)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
        }
    }
}

/// 补充标准库未覆盖的 IPv4 保留段（CGNAT、协议分配、基准测试等）。
fn is_reserved_v4(v4: Ipv4Addr) -> bool {
    matches!(
        v4.octets(),
        [100, 64..=127, _, _]  // 100.64.0.0/10 CGNAT
            | [192, 0, 0, _]   // 192.0.0.0/24 IETF 协议分配
            | [198, 18..=19, _, _] // 198.18.0.0/15 基准测试
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_scheme() {
        assert!(assert_public_http_url("file:///etc/passwd").is_err());
        assert!(assert_public_http_url("ftp://example.com").is_err());
    }

    #[test]
    fn rejects_private_literals() {
        for url in [
            "http://127.0.0.1/x",
            "http://10.0.0.1/x",
            "http://192.168.1.1/x",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/x",
        ] {
            assert!(
                assert_public_http_url(url).is_err(),
                "{url} should be blocked"
            );
        }
    }

    #[test]
    fn allows_public_host() {
        assert!(assert_public_http_url("https://example.com/list").is_ok());
    }
}
