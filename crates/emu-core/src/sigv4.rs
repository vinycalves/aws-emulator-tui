//! A small SigV4 signer.
//!
//! The emulator verifies signatures when it is started with `--auth on` (the
//! default), so every request this crate sends is signed. The implementation is
//! deliberately self-contained: it signs exactly the bytes that are sent on the
//! wire and keeps no global state.

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// The AWS percent-encoding set: everything except unreserved characters is
/// escaped. Used for canonical query strings and path segments.
const AWS_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Credentials read from the emulator's generated credentials file, the
/// environment, or the shared AWS credentials file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

impl Credentials {
    pub fn new(access_key_id: impl Into<String>, secret_access_key: impl Into<String>) -> Self {
        Self {
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
        }
    }

    pub fn with_session_token(mut self, token: impl Into<String>) -> Self {
        self.session_token = Some(token.into());
        self
    }
}

/// A header to be signed and sent. Names are sent as provided; canonicalization
/// lowercases them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Everything needed to sign one request.
pub struct SignableRequest<'a> {
    pub method: &'a str,
    /// Already percent-encoded path, starting with `/` (`Url::path`).
    pub path: &'a str,
    /// Already percent-encoded query string without the leading `?`.
    pub query: &'a str,
    /// Headers that will be sent, including `host`. A `host` header is added if
    /// the caller did not provide one.
    pub headers: Vec<Header>,
    pub body: &'a [u8],
    pub service: &'a str,
    pub region: &'a str,
    pub credentials: &'a Credentials,
    pub now: SystemTime,
}

/// The result of signing: the header list to send (now including
/// `Authorization`, `x-amz-date` and, when applicable, `x-amz-security-token`).
pub struct SignedRequest {
    pub headers: Vec<Header>,
}

/// Sign a request with AWS Signature Version 4.
pub fn sign(request: SignableRequest<'_>) -> SignedRequest {
    let amz_date = amz_date(request.now);
    let date_stamp = amz_date[..8].to_string();

    let mut headers = request.headers;
    set_header(&mut headers, "x-amz-date", &amz_date);
    if let Some(token) = &request.credentials.session_token {
        set_header(&mut headers, "x-amz-security-token", token);
    }
    if !headers.iter().any(|h| h.name.eq_ignore_ascii_case("host")) {
        // Callers normally supply it; this is a safety net.
        set_header(&mut headers, "host", "127.0.0.1");
    }

    let mut canonical_headers: Vec<(String, String)> = headers
        .iter()
        .map(|h| (h.name.to_ascii_lowercase(), normalize_value(&h.value)))
        .collect();
    canonical_headers.sort_by(|a, b| a.0.cmp(&b.0));
    canonical_headers.dedup_by(|a, b| a.0 == b.0);

    let signed_headers = canonical_headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{method}\n{path}\n{query}\n{headers}\n{signed_headers}\n{payload_hash}",
        method = request.method,
        path = request.path,
        query = canonical_query(request.query),
        headers = canonical_headers
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect::<String>(),
        payload_hash = hex::encode(Sha256::digest(request.body)),
    );

    let scope = format!(
        "{date_stamp}/{}/{}/aws4_request",
        request.region, request.service
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let signing_key = signing_key(
        &request.credentials.secret_access_key,
        &date_stamp,
        request.region,
        request.service,
    );
    let signature = hex::encode(hmac(&signing_key, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        request.credentials.access_key_id
    );
    set_header(&mut headers, "Authorization", &authorization);

    SignedRequest { headers }
}

fn set_header(headers: &mut Vec<Header>, name: &str, value: &str) {
    if let Some(existing) = headers
        .iter_mut()
        .find(|h| h.name.eq_ignore_ascii_case(name))
    {
        existing.value = value.to_string();
    } else {
        headers.push(Header::new(name, value));
    }
}

fn normalize_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Canonicalize a raw (already encoded) query string: decode each component,
/// re-encode with the AWS set, sort by name then value.
fn canonical_query(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<(String, String)> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let name = decode(parts.next().unwrap_or_default());
            let value = decode(parts.next().unwrap_or_default());
            (encode(&name), encode(&value))
        })
        .collect();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn decode(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

/// Percent-encode a single path segment or query component.
pub fn encode(value: &str) -> String {
    utf8_percent_encode(value, AWS_ENCODE_SET).to_string()
}

/// Encode an object key for a URL path, preserving `/` separators.
pub fn encode_path(path: &str) -> String {
    path.split('/').map(encode).collect::<Vec<_>>().join("/")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    hmac(&k_service, b"aws4_request")
}

/// Format a `SystemTime` as `YYYYMMDDTHHMMSSZ`.
pub fn amz_date(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;

    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// Howard Hinnant's `civil_from_days`, valid for the whole Unix epoch range.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_dates() {
        let epoch = UNIX_EPOCH;
        assert_eq!(amz_date(epoch), "19700101T000000Z");
        let t = epoch + Duration::from_secs(1_700_000_000);
        assert_eq!(amz_date(t), "20231114T221320Z");
        let leap = epoch + Duration::from_secs(951_782_400);
        assert_eq!(amz_date(leap), "20000229T000000Z");
    }

    #[test]
    fn canonicalizes_query() {
        assert_eq!(canonical_query("b=2&a=1"), "a=1&b=2");
        assert_eq!(canonical_query("list-type=2"), "list-type=2");
        assert_eq!(canonical_query("prefix=a%20b"), "prefix=a%20b");
        assert_eq!(canonical_query(""), "");
    }

    #[test]
    fn matches_reference_signature() {
        // Reference computed with botocore's canonical request and the standard
        // SigV4 HMAC chain for this exact request.
        let credentials = Credentials::new("EMULATOR123", "SECRET456");
        let signed = sign(SignableRequest {
            method: "POST",
            path: "/",
            query: "",
            headers: vec![
                Header::new("host", "127.0.0.1:9297"),
                Header::new("content-type", "application/x-amz-json-1.0"),
                Header::new("x-amz-target", "DynamoDB_20120810.ListTables"),
            ],
            body: b"{}",
            service: "dynamodb",
            region: "us-east-1",
            credentials: &credentials,
            now: UNIX_EPOCH + Duration::from_secs(1_767_323_045),
        });
        let authorization = signed
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case("authorization"))
            .expect("authorization header");
        assert_eq!(
            authorization.value,
            "AWS4-HMAC-SHA256 Credential=EMULATOR123/20260102/us-east-1/dynamodb/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date;x-amz-target, \
             Signature=cc7bc0ff8087eb489e2ee045974366617270ecdf04675823c0b7f07968f544bb"
                .replace("\\\n", "")
        );
    }

    #[test]
    fn encodes_paths() {
        assert_eq!(encode_path("a b/c+d"), "a%20b/c%2Bd");
        assert_eq!(encode("a~b_c-d.e"), "a~b_c-d.e");
    }
}
