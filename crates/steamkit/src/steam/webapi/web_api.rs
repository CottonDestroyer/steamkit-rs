//! Calling the Steam Web API.
//!
//! Interfaces have explicit methods ([`AsyncInterface::call`],
//! [`AsyncInterface::call_protobuf`], [`AsyncInterface::call_protobuf_with_request`])
//! rather than SteamKit2's dynamic `api.ISteamDirectory.GetCMListForConnect(...)`
//! calls. [`Interface`] is async as well.
//!
//! Arguments are an ordered [`WebApiArgs`] list, so the query string comes out
//! in a stable order.

use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use crate::base::generated::steam_language::EResult;
use crate::error::SteamError;
use crate::types::key_value::KeyValue;

use super::web_api_request_exception::WebAPIRequestException;

/// The default base address used for the Steam Web API.
pub const DEFAULT_BASE_ADDRESS: &str = "https://api.steampowered.com/";

/// The default timeout used for Web API requests.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(100);

/// The characters `Uri.EscapeDataString` leaves alone: `A-Z`, `a-z`, `0-9`,
/// `-`, `_`, `.` and `~`. `percent_encoding` has no constant for exactly this
/// set.
fn uri_unreserved() -> &'static AsciiSet {
    static SET: OnceLock<AsciiSet> = OnceLock::new();

    SET.get_or_init(|| {
        NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~')
    })
}

/// The value of a Web API argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebApiValue {
    /// Text; escaped like `Uri.EscapeDataString`.
    Text(String),
    /// Binary; every byte is escaped as `%XX`.
    Bytes(Vec<u8>),
    /// Left out of the request.
    Null,
}

impl From<String> for WebApiValue {
    /// A text value.
    fn from(value: String) -> Self {
        WebApiValue::Text(value)
    }
}

impl From<&str> for WebApiValue {
    /// A text value.
    fn from(value: &str) -> Self {
        WebApiValue::Text(value.to_string())
    }
}

impl From<Vec<u8>> for WebApiValue {
    /// A binary value.
    fn from(value: Vec<u8>) -> Self {
        WebApiValue::Bytes(value)
    }
}

impl From<i32> for WebApiValue {
    /// The number as text.
    fn from(value: i32) -> Self {
        WebApiValue::Text(value.to_string())
    }
}

impl From<u32> for WebApiValue {
    /// The number as text.
    fn from(value: u32) -> Self {
        WebApiValue::Text(value.to_string())
    }
}

impl From<i64> for WebApiValue {
    /// The number as text.
    fn from(value: i64) -> Self {
        WebApiValue::Text(value.to_string())
    }
}

impl From<u64> for WebApiValue {
    /// The number as text.
    fn from(value: u64) -> Self {
        WebApiValue::Text(value.to_string())
    }
}

impl From<bool> for WebApiValue {
    /// The boolean as text.
    fn from(value: bool) -> Self {
        WebApiValue::Text(value.to_string())
    }
}

/// A single argument of a Web API call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebApiArgument {
    /// The name of the argument.
    pub name: String,
    /// The value of the argument.
    pub value: WebApiValue,
}

impl WebApiArgument {
    /// Creates an argument with the given name and value.
    pub fn new(name: impl Into<String>, value: impl Into<WebApiValue>) -> Self {
        WebApiArgument {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// The arguments of a Web API call.
pub type WebApiArgs = Vec<WebApiArgument>;

/// Builds the argument list of a Web API call. The arguments keep their order
/// in the query string.
pub fn web_api_args<const N: usize>(entries: [(&str, WebApiValue); N]) -> WebApiArgs {
    entries
        .into_iter()
        .map(|(name, value)| WebApiArgument::new(name, value))
        .collect()
}

/// A response to a Web API request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebAPIResponse<TResponse> {
    /// The result of the response.
    pub result: EResult,
    /// The body of the response.
    pub body: TResponse,
}

impl<TResponse> WebAPIResponse<TResponse> {
    /// Creates a response.
    pub fn new(result: EResult, body: TResponse) -> Self {
        WebAPIResponse { result, body }
    }
}

/// Calling the Steam Web API.
pub struct WebAPI;

impl WebAPI {
    /// The default base address of the Steam Web API. Use a `SteamConfiguration`
    /// or the `_with_base_address` functions for a different one.
    pub fn default_base_address() -> reqwest::Url {
        // A valid constant, so this can't fail.
        reqwest::Url::parse(DEFAULT_BASE_ADDRESS)
            .expect("DEFAULT_BASE_ADDRESS is a valid absolute URL")
    }

    /// The default timeout used for Web API requests.
    pub fn default_timeout() -> Duration {
        DEFAULT_TIMEOUT
    }

    /// An interface for calling the given Web API interface.
    pub fn get_interface(iface: &str, api_key: &str) -> Interface {
        Interface::new(
            create_default_http_client(),
            Self::default_base_address(),
            iface,
            api_key,
        )
    }

    /// An interface for calling the given Web API interface at `base_address`.
    pub fn get_interface_with_base_address(
        base_address: reqwest::Url,
        iface: &str,
        api_key: &str,
    ) -> Interface {
        Interface::new(
            create_default_http_client(),
            base_address,
            iface,
            api_key,
        )
    }

    /// An async interface for calling the given Web API interface.
    pub fn get_async_interface(iface: &str, api_key: &str) -> AsyncInterface {
        AsyncInterface::new(
            create_default_http_client(),
            Self::default_base_address(),
            iface,
            api_key,
        )
    }

    /// An async interface for calling the given Web API interface at `base_address`.
    pub fn get_async_interface_with_base_address(
        base_address: reqwest::Url,
        iface: &str,
        api_key: &str,
    ) -> AsyncInterface {
        AsyncInterface::new(
            create_default_http_client(),
            base_address,
            iface,
            api_key,
        )
    }
}

/// The default HTTP client for Web API interfaces.
fn create_default_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// One interface of the Web API. Wraps an [`AsyncInterface`].
pub struct Interface {
    inner: Arc<AsyncInterface>,
}

impl Interface {
    /// Creates an interface for the given Web API interface.
    pub fn new(
        http_client: reqwest::Client,
        base_address: reqwest::Url,
        iface: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        Interface {
            inner: Arc::new(AsyncInterface::new(http_client, base_address, iface, api_key)),
        }
    }

    /// The async interface this wraps.
    pub fn async_interface(&self) -> &AsyncInterface {
        &self.inner
    }

    /// The name of the Web API interface, for example `ISteamDirectory`.
    pub fn iface(&self) -> &str {
        self.inner.iface()
    }

    /// The timeout for Web API requests.
    pub fn timeout(&self) -> Duration {
        self.inner.timeout()
    }

    /// Sets the timeout for Web API requests.
    pub fn set_timeout(&self, timeout: Duration) {
        self.inner.set_timeout(timeout);
    }

    /// Calls a Web API function.
    pub async fn call(
        &self,
        func: &str,
        version: i32,
        args: &[WebApiArgument],
    ) -> Result<KeyValue, SteamError> {
        self.inner.call(func, version, args).await
    }

    /// Calls a Web API function with the given HTTP method.
    pub async fn call_with_method(
        &self,
        method: reqwest::Method,
        func: &str,
        version: i32,
        args: &[WebApiArgument],
    ) -> Result<KeyValue, SteamError> {
        self.inner
            .call_with_method(method, func, version, args)
            .await
    }
}

/// One interface of the Web API.
pub struct AsyncInterface {
    http_client: reqwest::Client,
    base_address: reqwest::Url,
    iface: String,
    api_key: String,
    timeout_millis: AtomicU64,
}

impl AsyncInterface {
    /// Creates an interface for the given Web API interface.
    pub fn new(
        http_client: reqwest::Client,
        base_address: reqwest::Url,
        iface: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Self {
        AsyncInterface {
            http_client,
            base_address,
            iface: iface.into(),
            api_key: api_key.into(),
            timeout_millis: AtomicU64::new(DEFAULT_TIMEOUT.as_millis() as u64),
        }
    }

    /// The name of the Web API interface, for example `ISteamDirectory`.
    pub fn iface(&self) -> &str {
        &self.iface
    }

    /// The API key used for authorized requests.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// The base address of the Web API.
    pub fn base_address(&self) -> &reqwest::Url {
        &self.base_address
    }

    /// The timeout for Web API requests.
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_millis.load(Ordering::Relaxed))
    }

    /// Sets the timeout for Web API requests.
    pub fn set_timeout(&self, timeout: Duration) {
        self.timeout_millis
            .store(timeout.as_millis() as u64, Ordering::Relaxed);
    }

    /// Calls a Web API function. The response is requested as VDF and parsed
    /// into a [`KeyValue`].
    pub async fn call(
        &self,
        func: &str,
        version: i32,
        args: &[WebApiArgument],
    ) -> Result<KeyValue, SteamError> {
        self.call_with_method(reqwest::Method::GET, func, version, args)
            .await
    }

    /// Calls a Web API function with the given HTTP method. The response is
    /// requested as VDF and parsed into a [`KeyValue`].
    pub async fn call_with_method(
        &self,
        method: reqwest::Method,
        func: &str,
        version: i32,
        args: &[WebApiArgument],
    ) -> Result<KeyValue, SteamError> {
        let response = self.call_internal(method, func, version, args, "vdf").await?;
        let body = response
            .bytes()
            .await
            .map_err(|error| map_reqwest_error(error, self.timeout()))?;

        let mut key_value = KeyValue::new();
        let mut cursor = Cursor::new(body.as_ref());

        key_value
            .read_as_text(&mut cursor)
            .map(|()| key_value)
            .map_err(|error| {
                SteamError::Malformed(format!(
                    "An internal error occurred when attempting to parse the response from the WebAPI server. \
                     This can indicate a change in the VDF format. {error}"
                ))
            })
    }

    /// Calls a Web API function and parses the protobuf response.
    pub async fn call_protobuf<T>(
        &self,
        method: reqwest::Method,
        func: &str,
        version: i32,
        args: &[WebApiArgument],
    ) -> Result<T, SteamError>
    where
        T: prost::Message + Default,
    {
        let response = self
            .call_internal(method, func, version, args, "protobuf_raw")
            .await?;
        let body = response
            .bytes()
            .await
            .map_err(|error| map_reqwest_error(error, self.timeout()))?;

        T::decode(body.as_ref()).map_err(SteamError::Protobuf)
    }

    /// Calls a Web API function with a protobuf request and parses the protobuf
    /// response. The request goes base64-encoded in `input_protobuf_encoded`;
    /// the result code comes from the `x-eresult` response header.
    pub async fn call_protobuf_with_request<TResponse, TRequest>(
        &self,
        method: reqwest::Method,
        func: &str,
        request: &TRequest,
        version: i32,
        extra_args: &[WebApiArgument],
    ) -> Result<WebAPIResponse<TResponse>, SteamError>
    where
        TResponse: prost::Message + Default,
        TRequest: prost::Message + Default,
    {
        let base64 = BASE64_STANDARD.encode(request.encode_to_vec());

        let mut args: WebApiArgs = Vec::with_capacity(extra_args.len() + 1);
        args.push(WebApiArgument::new(
            "input_protobuf_encoded",
            WebApiValue::Text(base64),
        ));

        for argument in extra_args {
            if !args.iter().any(|existing| existing.name == argument.name) {
                args.push(argument.clone());
            }
        }

        let response = self
            .call_internal(method, func, version, &args, "protobuf_raw")
            .await?;

        let result = response
            .headers()
            .get("x-eresult")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i32>().ok())
            .and_then(EResult::from_value)
            .unwrap_or(EResult::Invalid);

        let body = response
            .bytes()
            .await
            .map_err(|error| map_reqwest_error(error, self.timeout()))?;
        let body = TResponse::decode(body.as_ref()).map_err(SteamError::Protobuf)?;

        Ok(WebAPIResponse::new(result, body))
    }

    /// Sends the request and returns the raw response.
    async fn call_internal(
        &self,
        method: reqwest::Method,
        func: &str,
        version: i32,
        args: &[WebApiArgument],
        expected_format: &str,
    ) -> Result<reqwest::Response, SteamError> {
        let format_provided = args.iter().any(|argument| argument.name == "format");

        if format_provided {
            let format = args
                .iter()
                .find(|argument| argument.name == "format")
                .map(|argument| argument.value.clone())
                .unwrap_or(WebApiValue::Null);

            match format {
                WebApiValue::Text(text) if text == expected_format => {}
                other => {
                    let text = match other {
                        WebApiValue::Text(text) => text,
                        WebApiValue::Bytes(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                        WebApiValue::Null => String::new(),
                    };

                    return Err(SteamError::InvalidArgument(format!(
                        "Unsupported 'format' value '{text}'. Format must either be '{expected_format}' or omitted."
                    )));
                }
            }
        }

        let params = build_params(&self.api_key, expected_format, format_provided, args);
        let params_go_in_query_string = method == reqwest::Method::GET;

        let relative = format!("{}/{func}/v{version}/", self.iface);

        let mut url = self
            .base_address
            .join(&relative)
            .map_err(|error| SteamError::InvalidArgument(format!("Invalid Web API url: {error}")))?;

        let request = if params_go_in_query_string {
            url.set_query(Some(&params));
            self.http_client.request(method.clone(), url)
        } else {
            self.http_client
                .request(method.clone(), url)
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(params)
        };

        let timeout = self.timeout();

        let response = match tokio::time::timeout(timeout, request.send()).await {
            Ok(result) => result.map_err(|error| map_reqwest_error(error, timeout))?,
            Err(_) => return Err(SteamError::Timeout(timeout)),
        };

        if !response.status().is_success() {
            return Err(WebAPIRequestException::status(
                response.status().as_u16(),
                response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Unknown")
                    .to_string(),
            )
            .into());
        }

        Ok(response)
    }
}

/// Builds the query string: the API key first, then the format (unless the
/// caller passed one), then every argument as `&name=value`.
fn build_params(
    api_key: &str,
    expected_format: &str,
    format_provided: bool,
    args: &[WebApiArgument],
) -> String {
    let mut params = String::new();

    if !api_key.is_empty() && !args.iter().any(|argument| argument.name == "key") {
        params.push_str("key=");
        params.push_str(&escape_data_string(api_key));
        params.push('&');
    }

    if !format_provided {
        params.push_str("format=");
        params.push_str(expected_format);
    }

    for argument in args {
        params.push('&');
        params.push_str(&escape_data_string(&argument.name));
        params.push('=');

        match &argument.value {
            WebApiValue::Null => {}
            WebApiValue::Bytes(bytes) => params.push_str(&url_encode_bytes(bytes)),
            WebApiValue::Text(text) => params.push_str(&escape_data_string(text)),
        }
    }

    params
}

/// Escapes a string like `Uri.EscapeDataString`.
pub(crate) fn escape_data_string(value: &str) -> String {
    utf8_percent_encode(value, uri_unreserved()).to_string()
}

/// Percent-encodes every byte (`%XX`).
fn url_encode_bytes(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 3);

    for byte in bytes {
        encoded.push('%');
        encoded.push_str(&format!("{byte:02X}"));
    }

    encoded
}

/// Maps a `reqwest` error to a [`SteamError`].
fn map_reqwest_error(error: reqwest::Error, timeout: Duration) -> SteamError {
    if error.is_timeout() {
        SteamError::Timeout(timeout)
    } else if error.is_connect() {
        SteamError::Connection(error.to_string())
    } else {
        SteamError::Other(error.to_string())
    }
}
