//! A small AWS API client aimed at one emulator endpoint.
//!
//! It speaks the four wire protocols the emulator implements (`json`, `query`,
//! `ec2`, `rest-json`) plus the S3 REST-XML calls the TUI needs. Requests are
//! signed with SigV4 using the credentials the emulator generates.

use std::time::{Duration, Instant, SystemTime};

use serde_json::{Map, Value};

use crate::catalog::{self, OpMeta, ServiceMeta};
use crate::config::ConnectionConfig;
use crate::sigv4::{self, Credentials, Header};
use crate::xml;

/// A successful (2xx/3xx) emulator response.
#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status: u16,
    pub body: String,
    pub json: Option<Value>,
    pub headers: Vec<(String, String)>,
    pub request_id: Option<String>,
    pub duration_ms: u128,
}

impl ApiResponse {
    pub fn pretty(&self) -> String {
        if let Some(json) = &self.json {
            serde_json::to_string_pretty(json).unwrap_or_else(|_| self.body.clone())
        } else {
            xml::pretty(&self.body)
        }
    }
}

/// An emulator error response, or a transport/protocol error.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ApiError {
    pub status: Option<u16>,
    pub code: Option<String>,
    pub message: String,
    pub body: String,
    pub request_id: Option<String>,
    pub duration_ms: u128,
}

impl ApiError {
    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            status: None,
            code: Some("TransportError".to_string()),
            message: message.into(),
            body: String::new(),
            request_id: None,
            duration_ms: 0,
        }
    }

    pub fn status_label(&self) -> String {
        match self.status {
            Some(status) => format!("HTTP {status}"),
            None => "transport".to_string(),
        }
    }
}

/// Client for one emulator endpoint.
#[derive(Clone)]
pub struct EmulatorClient {
    http: reqwest::Client,
    endpoint: String,
    host: String,
    region: String,
    credentials: Credentials,
}

impl EmulatorClient {
    pub fn new(config: &ConnectionConfig) -> anyhow::Result<Self> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(5))
            .http1_only();

        if let Some(pem) = &config.ca_pem {
            builder = builder.add_root_certificate(reqwest::Certificate::from_pem(pem)?);
        } else if is_loopback(&config.endpoint) {
            // The emulator serves a self-signed certificate. Accepting it for a
            // loopback endpoint is equivalent to trusting the generated CA file;
            // a remote endpoint always needs its CA.
            builder = builder.danger_accept_invalid_certs(true);
        }

        Ok(Self {
            http: builder.build()?,
            endpoint: config.endpoint.clone(),
            host: config.host(),
            region: config.region.clone(),
            credentials: config.credentials.clone(),
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    /// Health check served at `GET /__admin/health`.
    pub async fn health(&self) -> Result<(), String> {
        let url = format!("{}/__admin/health", self.endpoint);
        match self.http.get(url).send().await {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => Err(format!("health check returned HTTP {}", response.status())),
            Err(error) => Err(format!("cannot reach emulator: {error}")),
        }
    }

    /// Invoke one operation from the service catalog.
    pub async fn call(
        &self,
        service_key: &str,
        operation: &str,
        input: Value,
    ) -> Result<ApiResponse, ApiError> {
        let service = catalog::catalog()
            .get(service_key)
            .ok_or_else(|| ApiError::transport(format!("unknown service `{service_key}`")))?;
        let op = service.operation(operation).ok_or_else(|| {
            ApiError::transport(format!("unknown operation `{service_key}.{operation}`"))
        })?;

        match service.protocol() {
            "json" => self.call_json(service, op, operation, input).await,
            "query" | "ec2" => {
                self.call_query(service, operation, input, service.protocol() == "ec2")
                    .await
            }
            "rest-json" => self.call_rest(service, op, operation, input).await,
            "rest-xml" => Err(ApiError::transport(format!(
                "`{service_key}` is a REST-XML service; use the S3 actions or the raw console"
            ))),
            other => Err(ApiError::transport(format!(
                "unsupported protocol `{other}` for `{service_key}`"
            ))),
        }
    }

    async fn call_json(
        &self,
        service: &ServiceMeta,
        _op: &OpMeta,
        operation: &str,
        input: Value,
    ) -> Result<ApiResponse, ApiError> {
        let target = service.target.as_deref().unwrap_or_default();
        if target.is_empty() {
            return Err(ApiError::transport(format!(
                "`{}` has no JSON target prefix",
                service.display
            )));
        }
        let version = service.json_version.as_deref().unwrap_or("1.0");
        let mut headers = vec![
            Header::new("content-type", format!("application/x-amz-json-{version}")),
            Header::new("x-amz-target", format!("{target}.{operation}")),
        ];
        if service.query_compatible {
            headers.push(Header::new("x-amzn-query-mode", "true"));
        }
        let body = serde_json::to_vec(&input)
            .map_err(|error| ApiError::transport(format!("cannot encode input: {error}")))?;
        self.send(
            "POST",
            "/",
            "",
            headers,
            body,
            &service.signing,
            operation,
            &service.display,
        )
        .await
    }

    async fn call_query(
        &self,
        service: &ServiceMeta,
        operation: &str,
        input: Value,
        ec2: bool,
    ) -> Result<ApiResponse, ApiError> {
        let mut pairs: Vec<(String, String)> = Vec::new();
        pairs.push(("Action".to_string(), operation.to_string()));
        if let Some(version) = &service.version {
            pairs.push(("Version".to_string(), version.clone()));
        }
        flatten_query(&input, "", &mut pairs, ec2);
        let body = pairs
            .into_iter()
            .map(|(key, value)| format!("{}={}", sigv4::encode(&key), sigv4::encode(&value)))
            .collect::<Vec<_>>()
            .join("&");
        let headers = vec![Header::new(
            "content-type",
            "application/x-www-form-urlencoded; charset=utf-8",
        )];
        self.send(
            "POST",
            "/",
            "",
            headers,
            body.into_bytes(),
            &service.signing,
            operation,
            &service.display,
        )
        .await
    }

    async fn call_rest(
        &self,
        service: &ServiceMeta,
        op: &OpMeta,
        operation: &str,
        input: Value,
    ) -> Result<ApiResponse, ApiError> {
        let method = op.method.as_deref().unwrap_or("POST");
        let mut template = op.uri.clone().unwrap_or_else(|| "/".to_string());
        let mut remaining: Map<String, Value> = match input {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            other => {
                return Err(ApiError::transport(format!(
                    "REST operation input must be an object, got {other}"
                )))
            }
        };

        for (member, location) in &op.labels {
            if let Some(value) = remaining.remove(member) {
                let encoded = sigv4::encode(&scalar_to_string(&value));
                template = template.replace(&format!("{{{location}}}"), &encoded);
                template = template.replace(&format!("{{{member}}}"), &encoded);
            }
        }

        let mut query = Vec::new();
        for (member, location) in &op.query_members {
            if let Some(value) = remaining.remove(member) {
                match value {
                    Value::Array(items) => {
                        for item in items {
                            query.push((location.clone(), scalar_to_string(&item)));
                        }
                    }
                    other => query.push((location.clone(), scalar_to_string(&other))),
                }
            }
        }
        let query = query
            .into_iter()
            .map(|(key, value)| format!("{}={}", sigv4::encode(&key), sigv4::encode(&value)))
            .collect::<Vec<_>>()
            .join("&");

        let mut headers = Vec::new();
        for (member, location) in &op.header_members {
            if let Some(value) = remaining.remove(member) {
                headers.push(Header::new(location.clone(), scalar_to_string(&value)));
            }
        }

        let body_value = if let Some(payload) = &op.payload {
            remaining.remove(payload).unwrap_or(Value::Null)
        } else if remaining.is_empty() {
            Value::Null
        } else {
            Value::Object(remaining)
        };
        let body = match body_value {
            Value::Null => Vec::new(),
            value => serde_json::to_vec(&value)
                .map_err(|error| ApiError::transport(format!("cannot encode input: {error}")))?,
        };
        if !body.is_empty() {
            headers.push(Header::new("content-type", "application/json"));
        }

        self.send(
            method,
            &template,
            &query,
            headers,
            body,
            &service.signing,
            operation,
            &service.display,
        )
        .await
    }

    /// Send one S3 REST-XML request.
    pub async fn s3(
        &self,
        method: &str,
        path: &str,
        query: &str,
        body: Vec<u8>,
    ) -> Result<ApiResponse, ApiError> {
        let mut headers = Vec::new();
        if !body.is_empty() {
            headers.push(Header::new("content-type", "application/xml"));
        }
        self.send(
            method,
            path,
            query,
            headers,
            body,
            "s3",
            method,
            "Amazon S3",
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send(
        &self,
        method: &str,
        path: &str,
        query: &str,
        headers: Vec<Header>,
        body: Vec<u8>,
        signing_service: &str,
        operation: &str,
        display: &str,
    ) -> Result<ApiResponse, ApiError> {
        let url = if query.is_empty() {
            format!("{}{}", self.endpoint, path)
        } else {
            format!("{}{}?{}", self.endpoint, path, query)
        };
        let url = reqwest::Url::parse(&url).map_err(|error| {
            ApiError::transport(format!("invalid request URL `{url}`: {error}"))
        })?;

        let mut headers = headers;
        headers.push(Header::new("host", self.host.clone()));
        let signed = sigv4::sign(sigv4::SignableRequest {
            method,
            path: url.path(),
            query: url.query().unwrap_or(""),
            headers,
            body: &body,
            service: signing_service,
            region: &self.region,
            credentials: &self.credentials,
            now: SystemTime::now(),
        });

        let mut request = self.http.request(
            reqwest::Method::from_bytes(method.as_bytes()).map_err(|error| {
                ApiError::transport(format!("invalid method `{method}`: {error}"))
            })?,
            url,
        );
        for header in &signed.headers {
            request = request.header(&header.name, &header.value);
        }
        request = request.body(body);

        let started = Instant::now();
        let response = request.send().await.map_err(|error| ApiError {
            status: None,
            code: Some("TransportError".to_string()),
            message: format!("{display} {operation}: {error}"),
            body: String::new(),
            request_id: None,
            duration_ms: started.elapsed().as_millis(),
        })?;

        let status = response.status().as_u16();
        let response_headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    value.to_str().unwrap_or("<binary>").to_string(),
                )
            })
            .collect();
        let request_id = response_headers
            .iter()
            .find(|(name, _)| {
                name.eq_ignore_ascii_case("x-amzn-requestid")
                    || name.eq_ignore_ascii_case("x-amz-request-id")
            })
            .map(|(_, value)| value.clone());
        let text = response.text().await.map_err(|error| ApiError {
            status: Some(status),
            code: Some("ReadError".to_string()),
            message: format!("cannot read response body: {error}"),
            body: String::new(),
            request_id: request_id.clone(),
            duration_ms: started.elapsed().as_millis(),
        })?;
        let duration_ms = started.elapsed().as_millis();
        let json = parse_json_body(&text);

        if (200..400).contains(&status) {
            Ok(ApiResponse {
                status,
                body: text,
                json,
                headers: response_headers,
                request_id,
                duration_ms,
            })
        } else {
            let (code, message) = error_details(&text);
            Err(ApiError {
                status: Some(status),
                code,
                message: message.unwrap_or_else(|| {
                    if text.trim().is_empty() {
                        format!("{display} {operation} failed with HTTP {status}")
                    } else {
                        text.trim().to_string()
                    }
                }),
                body: text,
                request_id,
                duration_ms,
            })
        }
    }
}

fn parse_json_body(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

fn error_details(body: &str) -> (Option<String>, Option<String>) {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        let code = value
            .get("__type")
            .or_else(|| value.get("code"))
            .or_else(|| value.get("Code"))
            .and_then(Value::as_str)
            .map(|code| {
                code.rsplit_once('#')
                    .map(|(_, short)| short.to_string())
                    .unwrap_or_else(|| code.to_string())
            });
        let message = value
            .get("message")
            .or_else(|| value.get("Message"))
            .and_then(Value::as_str)
            .map(str::to_string);
        return (code, message);
    }
    if let Some(document) = xml::parse(body) {
        let root = document.root_element();
        let scope = xml::child(root, "Error").unwrap_or(root);
        let code = xml::child(scope, "Code")
            .map(xml::text)
            .filter(|value| !value.is_empty());
        let message = xml::child(scope, "Message")
            .map(xml::text)
            .filter(|value| !value.is_empty());
        if code.is_some() || message.is_some() {
            return (code, message);
        }
    }
    (None, None)
}

/// Flatten JSON into query-protocol form fields.
///
/// Without a shape model the serializer can only apply conventions: lists use
/// `.member.N` (query) or `.N` (EC2), maps use `.entry.N.key`/`.entry.N.value`
/// when the caller supplies an array of `{"key":..,"value":..}` objects.
fn flatten_query(value: &Value, prefix: &str, out: &mut Vec<(String, String)>, ec2: bool) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let next = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_query(value, &next, out, ec2);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let next = if ec2 {
                    format!("{prefix}.{}", index + 1)
                } else {
                    format!("{prefix}.member.{}", index + 1)
                };
                flatten_query(item, &next, out, ec2);
            }
        }
        Value::Null => {}
        scalar => out.push((prefix.to_string(), scalar_to_string(scalar))),
    }
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn is_loopback(endpoint: &str) -> bool {
    let host = endpoint
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(endpoint);
    let host = host.split('/').next().unwrap_or(host);
    let host = host.rsplit_once(':').map(|(host, _)| host).unwrap_or(host);
    host == "127.0.0.1" || host == "localhost" || host == "::1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_query_lists() {
        let value = serde_json::json!({
            "QueueName": "q",
            "Attributes": {"VisibilityTimeout": "30"},
            "Tags": [{"Key": "a", "Value": "b"}]
        });
        let mut out = Vec::new();
        flatten_query(&value, "", &mut out, false);
        assert!(out.contains(&("QueueName".to_string(), "q".to_string())));
        assert!(out.contains(&("Attributes.VisibilityTimeout".to_string(), "30".to_string())));
        assert!(out.contains(&("Tags.member.1.Key".to_string(), "a".to_string())));
    }

    #[test]
    fn flattens_ec2_lists_positionally() {
        let value = serde_json::json!({"InstanceIds": ["i-1", "i-2"]});
        let mut out = Vec::new();
        flatten_query(&value, "", &mut out, true);
        assert!(out.contains(&("InstanceIds.1".to_string(), "i-1".to_string())));
        assert!(out.contains(&("InstanceIds.2".to_string(), "i-2".to_string())));
    }

    #[test]
    fn reads_json_errors() {
        let (code, message) = error_details(
            r#"{"__type":"com.amazon.coral.service#ValidationException","message":"bad"}"#,
        );
        assert_eq!(code.as_deref(), Some("ValidationException"));
        assert_eq!(message.as_deref(), Some("bad"));
    }

    #[test]
    fn reads_xml_errors() {
        let (code, message) = error_details(
            "<ErrorResponse><Error><Code>NoSuchBucket</Code><Message>nope</Message></Error></ErrorResponse>",
        );
        assert_eq!(code.as_deref(), Some("NoSuchBucket"));
        assert_eq!(message.as_deref(), Some("nope"));
    }
}
