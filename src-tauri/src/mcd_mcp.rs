use crate::model::{McdMcpConfig, OperationResult};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const ENDPOINT: &str = "https://mcp.mcd.cn";
pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const TOKEN_ACCOUNT: &str = "mcd-mcp.token.v1";
const SESSION_HEADER: &str = "mcp-session-id";
const REQUIRED_TOOLS: &[&str] = &[
    "delivery-query-addresses",
    "delivery-query-stores",
    "query-meals",
    "query-meal-detail",
    "query-store-coupons",
    "calculate-price",
    "create-order",
    "query-order",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub latency_ms: u128,
    pub server_name: String,
    pub protocol_version: String,
    pub tools: Vec<String>,
}

pub fn validate(config: &McdMcpConfig) -> Result<(), String> {
    if config.endpoint.trim_end_matches('/') != ENDPOINT {
        return Err("为避免 MCP Token 泄露，服务地址必须使用麦当劳官方 https://mcp.mcd.cn".into());
    }
    if config.protocol_version != PROTOCOL_VERSION {
        return Err("当前点餐流程只支持 MCP 协议 2025-06-18".into());
    }
    Ok(())
}

pub struct Client {
    http: reqwest::Client,
    endpoint: String,
    token: String,
    protocol_version: String,
    session_id: Option<String>,
    next_id: u64,
}

impl Client {
    pub fn new(config: &McdMcpConfig, token: &str) -> Result<Self, String> {
        validate(config)?;
        let token = token.trim();
        if token.is_empty() {
            return Err("麦当劳 MCP Token 为空".into());
        }
        if token.to_ascii_lowercase().starts_with("bearer ") {
            return Err("MCP Token 请直接填写，不要添加 Bearer 前缀".into());
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("创建 MCP HTTP 客户端失败：{error}"))?;
        Ok(Self {
            http,
            endpoint: config.endpoint.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            protocol_version: config.protocol_version.clone(),
            session_id: None,
            next_id: 1,
        })
    }

    #[cfg(test)]
    fn for_test(endpoint: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            endpoint,
            token: "test-token".into(),
            protocol_version: PROTOCOL_VERSION.into(),
            session_id: None,
            next_id: 1,
        }
    }

    fn request(&self) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .post(&self.endpoint)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream");
        if let Some(session_id) = &self.session_id {
            request = request
                .header(SESSION_HEADER, session_id)
                .header("mcp-protocol-version", &self.protocol_version);
        }
        request
    }

    async fn send(
        &mut self,
        method: &str,
        params: Value,
        notification: bool,
    ) -> Result<Option<Value>, String> {
        let id = self.next_id;
        self.next_id += 1;
        let body = if notification {
            json!({"jsonrpc":"2.0","method":method,"params":params})
        } else {
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        };
        let response = self
            .request()
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("麦当劳 MCP 请求失败：{error}"))?;
        if self.session_id.is_none() {
            self.session_id = response
                .headers()
                .get(SESSION_HEADER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
        }
        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("读取麦当劳 MCP 响应失败：{error}"))?;
        if !status.is_success() {
            let detail = String::from_utf8_lossy(&bytes);
            let hint = match status.as_u16() {
                401 | 403 => "请检查 MCP Token 是否有效",
                429 => "请求过于频繁，请稍后重试",
                _ => "请稍后重试",
            };
            let detail = if matches!(status.as_u16(), 401 | 403) {
                String::new()
            } else {
                safe_server_detail(&detail)
            };
            return Err(format!("麦当劳 MCP 返回 HTTP {status}：{hint}{detail}"));
        }
        if bytes.is_empty() {
            return Ok(None);
        }
        let envelope = parse_response(&bytes, &content_type)?;
        if let Some(error) = envelope.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知 MCP 错误");
            let code = error.get("code").map(Value::to_string).unwrap_or_default();
            return Err(format!("麦当劳 MCP 调用失败：{message} {code}"));
        }
        Ok(Some(envelope.get("result").cloned().unwrap_or(envelope)))
    }

    pub async fn initialize(&mut self) -> Result<Value, String> {
        let result = self
            .send(
                "initialize",
                json!({
                    "protocolVersion":self.protocol_version,
                    "capabilities":{},
                    "clientInfo":{"name":"easyinput","version":env!("CARGO_PKG_VERSION")}
                }),
                false,
            )
            .await?
            .ok_or_else(|| "麦当劳 MCP 初始化没有返回结果".to_string())?;
        let negotiated = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !negotiated.is_empty() && negotiated != self.protocol_version {
            return Err(format!(
                "麦当劳 MCP 协议版本不兼容：服务端返回 {negotiated}"
            ));
        }
        self.send("notifications/initialized", json!({}), true)
            .await?;
        Ok(result)
    }

    pub async fn list_tools(&mut self) -> Result<Vec<String>, String> {
        let result = self
            .send("tools/list", json!({}), false)
            .await?
            .ok_or_else(|| "tools/list 没有返回结果".to_string())?;
        Ok(result
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect())
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        self.send(
            "tools/call",
            json!({"name":name,"arguments":arguments}),
            false,
        )
        .await?
        .ok_or_else(|| format!("工具 {name} 没有返回结果"))
    }

    pub fn has_session_id(&self) -> bool {
        self.session_id.is_some()
    }
}

pub async fn test_connection(
    config: &McdMcpConfig,
    token: &str,
) -> OperationResult<ConnectionTest> {
    let started = Instant::now();
    let mut client = match Client::new(config, token) {
        Ok(value) => value,
        Err(error) => return OperationResult::failure(error),
    };
    let initialized = match client.initialize().await {
        Ok(value) => value,
        Err(error) => return OperationResult::failure(error),
    };
    let tools = match client.list_tools().await {
        Ok(value) => value,
        Err(error) => return OperationResult::failure(error),
    };
    let missing = REQUIRED_TOOLS
        .iter()
        .filter(|required| !tools.iter().any(|tool| tool == **required))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return OperationResult::failure(format!(
            "MCP 已连接，但缺少点餐所需工具：{}",
            missing.join("、")
        ));
    }
    let server_name = initialized
        .pointer("/serverInfo/name")
        .and_then(Value::as_str)
        .unwrap_or("McDonald's MCP")
        .to_owned();
    let protocol_version = initialized
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION)
        .to_owned();
    OperationResult::success(Some(ConnectionTest {
        latency_ms: started.elapsed().as_millis(),
        server_name,
        protocol_version,
        tools,
    }))
}

fn safe_server_detail(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        String::new()
    } else {
        format!("（{}）", value.chars().take(160).collect::<String>())
    }
}

fn parse_response(bytes: &[u8], content_type: &str) -> Result<Value, String> {
    if content_type
        .to_ascii_lowercase()
        .contains("text/event-stream")
    {
        let text = std::str::from_utf8(bytes).map_err(|_| "MCP SSE 响应不是 UTF-8".to_string())?;
        let mut fallback = None;
        for line in text.lines().filter_map(|line| line.strip_prefix("data:")) {
            let data = line.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(data) {
                if value.get("result").is_some() || value.get("error").is_some() {
                    return Ok(value);
                }
                fallback = Some(value);
            }
        }
        if let Some(value) = fallback {
            return Ok(value);
        }
        return Err("MCP SSE 响应中没有 JSON data 事件".into());
    }
    serde_json::from_slice(bytes).map_err(|error| format!("MCP 响应 JSON 无效：{error}"))
}

/// Extract the business payload from a standard MCP tool result. Some servers
/// return structuredContent, while others place JSON text in content[].text.
pub fn tool_payload(result: &Value) -> Result<Value, String> {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        let message = result
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or("麦当劳工具返回业务错误");
        return Err(message.to_owned());
    }
    if let Some(value) = result.get("structuredContent") {
        return Ok(value.get("data").cloned().unwrap_or_else(|| value.clone()));
    }
    if let Some(text) = result.pointer("/content/0/text").and_then(Value::as_str) {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            return Ok(value.get("data").cloned().unwrap_or(value));
        }
        return Ok(json!({"message":text}));
    }
    Ok(result
        .get("data")
        .cloned()
        .unwrap_or_else(|| result.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[test]
    fn parses_json_and_sse() {
        assert_eq!(
            parse_response(
                br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#,
                "application/json"
            )
            .unwrap()
            .pointer("/result/ok"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            parse_response(
                b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"result\":{\"n\":2}}\n\n",
                "text/event-stream"
            )
            .unwrap()
            .pointer("/result/n")
            .and_then(Value::as_i64),
            Some(2)
        );
    }

    #[test]
    fn unwraps_structured_and_text_payloads() {
        assert_eq!(
            tool_payload(&json!({"structuredContent":{"data":{"x":1}}})).unwrap()["x"],
            1
        );
        assert_eq!(
            tool_payload(&json!({"content":[{"text":"{\"data\":{\"x\":2}}"}]})).unwrap()["x"],
            2
        );
    }

    #[tokio::test]
    async fn carries_session_and_protocol_headers_across_streamable_http_lifecycle() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (request_tx, request_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let responses = [
                ("200 OK", Some("test-session"), json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":PROTOCOL_VERSION,"serverInfo":{"name":"mock-mcd"}}}).to_string()),
                ("202 Accepted", None, String::new()),
                ("200 OK", None, json!({"jsonrpc":"2.0","id":3,"result":{"tools":REQUIRED_TOOLS.iter().map(|name|json!({"name":name})).collect::<Vec<_>>()}}).to_string()),
            ];
            for (status, session, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let size = stream.read(&mut buffer).unwrap();
                    if size == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..size]);
                    let header_end = bytes
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .map(|index| index + 4);
                    if let Some(header_end) = header_end {
                        let headers =
                            String::from_utf8_lossy(&bytes[..header_end]).to_ascii_lowercase();
                        let content_length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if bytes.len() >= header_end + content_length {
                            break;
                        }
                    }
                }
                request_tx
                    .send(String::from_utf8_lossy(&bytes).to_string())
                    .unwrap();
                let session_header = session
                    .map(|value| format!("Mcp-Session-Id: {value}\r\n"))
                    .unwrap_or_default();
                write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{session_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });

        let mut client = Client::for_test(endpoint);
        client.initialize().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert!(tools.contains(&"create-order".to_string()));
        let requests = [
            request_rx.recv().unwrap(),
            request_rx.recv().unwrap(),
            request_rx.recv().unwrap(),
        ];
        assert!(requests[0].contains("\"method\":\"initialize\""));
        assert!(requests[1].contains("\"method\":\"notifications/initialized\""));
        assert!(requests[2].contains("\"method\":\"tools/list\""));
        for request in &requests[1..] {
            let lower = request.to_ascii_lowercase();
            assert!(lower.contains("mcp-session-id: test-session"));
            assert!(lower.contains("mcp-protocol-version: 2025-06-18"));
        }
        server.join().unwrap();
    }
}
