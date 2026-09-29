use chrono::{Duration as ChronoDuration, Utc};
use qrcode::{render::svg, QrCode};
use serde::Serialize;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    thread,
    time::{Duration, Instant},
};
use url::Url;

const PAGE_LIFETIME: Duration = Duration::from_secs(15 * 60);
const MAX_URL_LENGTH: usize = 8 * 1024;
const MAX_REQUEST_HEADER_LENGTH: usize = 16 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentMvpLaunch {
    pub page_url: String,
    pub payment_host: String,
    pub expires_at: String,
    pub looks_like_mcd_domain: bool,
}

pub fn launch(pay_h5_url: &str) -> Result<PaymentMvpLaunch, String> {
    let payment_url = validate_payment_url(pay_h5_url)?;
    let payment_host = payment_url
        .host_str()
        .ok_or_else(|| "支付链接缺少域名".to_string())?
        .to_owned();
    let looks_like_mcd_domain = payment_host == "mcd.cn" || payment_host.ends_with(".mcd.cn");
    let qr_svg = QrCode::new(payment_url.as_str().as_bytes())
        .map_err(|error| format!("支付链接过长，无法生成二维码：{error}"))?
        .render::<svg::Color>()
        .min_dimensions(360, 360)
        .quiet_zone(true)
        .build();

    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let route = format!("/pay/{nonce}");
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("无法启动本机支付验证页：{error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("无法配置本机支付验证页：{error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("无法读取支付验证页端口：{error}"))?
        .port();
    let page_url = format!("http://127.0.0.1:{port}{route}");
    let expires_at =
        (Utc::now() + ChronoDuration::from_std(PAGE_LIFETIME).unwrap_or_default()).to_rfc3339();
    let html = payment_page(
        payment_url.as_str(),
        &payment_host,
        &qr_svg,
        &expires_at,
        looks_like_mcd_domain,
    );

    thread::Builder::new()
        .name("easyinput-payment-mvp".into())
        .spawn(move || serve_page(listener, route, html, PAGE_LIFETIME))
        .map_err(|error| format!("无法启动支付验证页线程：{error}"))?;

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/open")
            .arg(&page_url)
            .status()
            .map_err(|error| format!("无法打开默认浏览器：{error}"))?;
        if !status.success() {
            return Err(format!("默认浏览器启动失败：{status}"));
        }
    }
    #[cfg(not(target_os = "macos"))]
    return Err("支付 H5 MVP 当前仅支持 macOS".into());

    Ok(PaymentMvpLaunch {
        page_url,
        payment_host,
        expires_at,
        looks_like_mcd_domain,
    })
}

fn validate_payment_url(raw: &str) -> Result<Url, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("请粘贴 create-order 返回的 payH5Url".into());
    }
    if raw.len() > MAX_URL_LENGTH {
        return Err("支付链接超过 8 KB，已拒绝处理".into());
    }
    let url = Url::parse(raw).map_err(|error| format!("支付链接格式无效：{error}"))?;
    if url.scheme() != "https" {
        return Err("支付链接必须使用 HTTPS".into());
    }
    if url.host_str().is_none() {
        return Err("支付链接缺少域名".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("支付链接不能包含 URL 用户名或密码".into());
    }
    Ok(url)
}

fn serve_page(listener: TcpListener, route: String, html: String, lifetime: Duration) {
    let deadline = Instant::now() + lifetime;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = respond(&mut stream, &route, &html);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
}

fn respond(stream: &mut TcpStream, route: &str, html: &str) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request_bytes = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let size = stream.read(&mut chunk)?;
        if size == 0 {
            break;
        }
        request_bytes.extend_from_slice(&chunk[..size]);
        if request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request_bytes.len() > MAX_REQUEST_HEADER_LENGTH {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP request headers are too large",
            ));
        }
    }
    let request = String::from_utf8_lossy(&request_bytes);
    let requested_path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    if requested_path == route {
        write_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            html.as_bytes(),
        )
    } else if requested_path == "/favicon.ico" {
        write_response(stream, "204 No Content", "image/x-icon", &[])
    } else {
        write_response(
            stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "支付验证页不存在或已过期".as_bytes(),
        )
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store, max-age=0\r\nPragma: no-cache\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)
}

fn payment_page(
    payment_url: &str,
    payment_host: &str,
    qr_svg: &str,
    expires_at: &str,
    looks_like_mcd_domain: bool,
) -> String {
    let escaped_url = escape_html(payment_url);
    let escaped_host = escape_html(payment_host);
    let escaped_expires_at = escape_html(expires_at);
    let domain_note = if looks_like_mcd_domain {
        "链接域名符合 mcd.cn 形式；支付前仍请核对浏览器最终跳转域名和金额。"
    } else {
        "该链接不是 mcd.cn 域名。支付服务可能使用合作方域名，请只在确认它确实来自本次 MCP 返回后继续。"
    };
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>EasyInput 支付 H5 验证</title>
  <style>
    :root {{ color-scheme: light; font-family: -apple-system,BlinkMacSystemFont,"PingFang SC",sans-serif; color:#292520; background:#f5f2ec; }}
    * {{ box-sizing:border-box; }} body {{ margin:0; min-height:100vh; display:grid; place-items:center; padding:28px; }}
    main {{ width:min(760px,100%); background:#fff; border:1px solid #ded8cf; border-radius:18px; padding:34px; box-shadow:0 24px 80px rgba(63,43,28,.12); }}
    .eyebrow {{ color:#b75336; font-size:12px; letter-spacing:.12em; }} h1 {{ margin:10px 0 8px; font:32px STSong,Songti SC,serif; }}
    .lead {{ color:#6f675f; line-height:1.7; margin:0 0 24px; }} .layout {{ display:grid; grid-template-columns:390px 1fr; gap:30px; align-items:center; }}
    .qr {{ padding:14px; border:1px solid #e4dfd8; border-radius:14px; background:#fff; }} .qr svg {{ width:100%; height:auto; display:block; }}
    dl {{ margin:0; }} dl div {{ padding:12px 0; border-bottom:1px solid #eee9e2; }} dt {{ color:#968e85; font-size:11px; }} dd {{ margin:5px 0 0; overflow-wrap:anywhere; }}
    .warning {{ margin:22px 0; padding:13px 15px; border-left:3px solid #b75336; background:#f8efe9; color:#6c5146; font-size:13px; line-height:1.7; }}
    .actions {{ display:flex; gap:10px; flex-wrap:wrap; }} a {{ display:inline-flex; align-items:center; justify-content:center; min-height:42px; padding:0 18px; border-radius:8px; text-decoration:none; }}
    .primary {{ background:#b75336; color:#fff; }} .secondary {{ border:1px solid #d9d2c9; color:#403a34; }}
    ol {{ margin:26px 0 0; padding-left:20px; color:#665f57; font-size:13px; line-height:1.8; }}
    @media(max-width:720px) {{ .layout {{ grid-template-columns:1fr; }} .qr {{ max-width:390px; margin:auto; }} main {{ padding:24px; }} }}
  </style>
</head>
<body>
  <main>
    <div class="eyebrow">PAY H5 COMPATIBILITY MVP</div>
    <h1>麦当劳支付链接验证</h1>
    <p class="lead">用手机扫描二维码，确认是否能够进入本次订单的官方支付页面。本页不会自动支付，也不会把链接上传到其他服务。</p>
    <div class="layout">
      <div class="qr">{qr_svg}</div>
      <dl>
        <div><dt>目标域名</dt><dd>{escaped_host}</dd></div>
        <div><dt>本机验证页失效时间</dt><dd>{escaped_expires_at}</dd></div>
        <div><dt>验证重点</dt><dd>跨设备打开、微信/支付宝环境、登录态、订单金额与支付成功回跳</dd></div>
      </dl>
    </div>
    <div class="warning">{domain_note}</div>
    <div class="actions">
      <a class="primary" href="{escaped_url}" target="_blank" rel="noreferrer noopener">在当前设备直接打开支付页</a>
      <a class="secondary" href="https://open.mcd.cn/mcp" target="_blank" rel="noreferrer noopener">打开麦当劳 MCP 平台</a>
    </div>
    <ol>
      <li>先记录扫码使用的 App 和网络环境。</li>
      <li>核对最终域名、订单金额、订单号和支付截止时间。</li>
      <li>如果只是验证页面兼容性，不要点击最终付款。</li>
      <li>记录是否要求重新登录、是否提示必须在微信内打开，以及支付完成后的回跳行为。</li>
    </ol>
  </main>
</body>
</html>"#
    )
}

fn escape_html(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&#39;".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Shutdown, TcpStream};

    #[test]
    fn rejects_non_https_payment_urls() {
        assert!(validate_payment_url("http://pay.mcd.cn/order/1")
            .unwrap_err()
            .contains("HTTPS"));
        assert!(validate_payment_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn accepts_https_url_and_preserves_fragment() {
        let url = validate_payment_url("https://pay.mcd.cn/h5?a=1&b=2#/cashier").unwrap();
        assert_eq!(url.as_str(), "https://pay.mcd.cn/h5?a=1&b=2#/cashier");
    }

    #[test]
    fn page_does_not_embed_remote_payment_site() {
        let page = payment_page(
            "https://pay.mcd.cn/h5?a=1&b=2",
            "pay.mcd.cn",
            "<svg></svg>",
            "2026-09-02T12:00:00Z",
            true,
        );
        assert!(!page.contains("<iframe"));
        assert!(page.contains("rel=\"noreferrer noopener\""));
        assert!(page.contains("https://pay.mcd.cn/h5?a=1&amp;b=2"));
    }

    #[test]
    fn loopback_page_uses_no_store_and_rejects_unknown_routes() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                respond(&mut stream, "/pay/test", "<html>payment</html>").unwrap();
            }
        });

        let response = loopback_request(address, "/pay/test");
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response:?}");
        assert!(response.contains("Cache-Control: no-store"));
        assert!(response.contains("Content-Security-Policy: default-src 'none'"));
        assert!(response.ends_with("<html>payment</html>"));

        let missing = loopback_request(address, "/pay/missing");
        assert!(missing.starts_with("HTTP/1.1 404 Not Found"), "{missing:?}");
        server.join().unwrap();
    }

    fn loopback_request(address: std::net::SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();

        let mut response_bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(size) => response_bytes.extend_from_slice(&chunk[..size]),
                // macOS may report ECONNRESET after all response bytes were received
                // when the short-lived loopback server closes the connection.
                Err(error)
                    if error.kind() == std::io::ErrorKind::ConnectionReset
                        && !response_bytes.is_empty() =>
                {
                    break;
                }
                Err(error) => panic!("failed to read loopback response: {error}"),
            }
        }
        String::from_utf8(response_bytes).unwrap()
    }
}
