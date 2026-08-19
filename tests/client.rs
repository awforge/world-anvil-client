use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
use world_anvil_client::{
    Client, Credentials,
    api::{self, ClientCategoryExt},
};

const APPLICATION_KEY: &str = "application-key";
const AUTH_TOKEN: &str = "auth-token";
const WORLD_ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const CATEGORY_ID: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const SERVER_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test(flavor = "current_thread")]
async fn create_category_sends_credentials_and_body_and_decodes_success() {
    // Arrange
    let server = MockServer::start(MockResponse::json(
        "200 OK",
        format!(r#"{{"id":"{CATEGORY_ID}","title":"Test Category"}}"#),
    ));
    let client = test_client(&server);

    // Act
    let response = client
        .create_category()
        .body(category_body())
        .send()
        .await
        .expect("the mock response should decode");

    // Assert
    assert_eq!(response.status(), StatusCode::OK);
    let category = response.into_inner();
    assert_eq!(category.id, Some(Uuid::parse_str(CATEGORY_ID).unwrap()));
    assert_eq!(category.title.as_deref(), Some("Test Category"));
    assert_create_category_request(&server.finish());
}

#[tokio::test(flavor = "current_thread")]
async fn create_category_preserves_a_bodyless_documented_error() {
    // Arrange
    let server = MockServer::start(MockResponse::empty("401 Unauthorized"));
    let client = test_client(&server);

    // Act
    let error = client
        .create_category()
        .body(category_body())
        .send()
        .await
        .expect_err("the mock response should be an API error");

    // Assert
    match error {
        api::Error::ErrorResponse(response) => {
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.content_length(), Some(0));
        }
        other => panic!("expected a documented error response, got {other:?}"),
    }
    assert_create_category_request(&server.finish());
}

fn test_client(server: &MockServer) -> Client {
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("the test HTTP client should build");
    Client::new_with_client(
        server.base_url(),
        http_client,
        Credentials::new(APPLICATION_KEY, AUTH_TOKEN)
            .expect("test credentials are valid header values"),
    )
}

fn category_body() -> api::types::CategoryCreate {
    let world_id = Uuid::parse_str(WORLD_ID).expect("the test world UUID is valid");
    api::types::CategoryCreate::builder()
        .title("Test Category")
        .world(api::types::CategoryCreateWorld::builder().id(Some(world_id)))
        .try_into()
        .expect("the category fixture is valid")
}

fn assert_create_category_request(request: &CapturedRequest) {
    assert_eq!(request.method, "PUT");
    assert_eq!(request.target, "/category");
    assert_eq!(request.header("x-application-key"), APPLICATION_KEY);
    assert_eq!(request.header("x-auth-token"), AUTH_TOKEN);
    assert_eq!(request.header("api-version"), "2.0.0 - Boromir");
    assert_eq!(request.header("content-type"), "application/json");

    let body: Value =
        serde_json::from_slice(&request.body).expect("request body should be valid JSON");
    assert_eq!(
        body,
        json!({
            "title": "Test Category",
            "world": { "id": WORLD_ID },
        })
    );
}

struct MockResponse {
    status: &'static str,
    content_type: Option<&'static str>,
    body: Vec<u8>,
}

impl MockResponse {
    fn json(status: &'static str, body: String) -> Self {
        Self {
            status,
            content_type: Some("application/json"),
            body: body.into_bytes(),
        }
    }

    fn empty(status: &'static str) -> Self {
        Self {
            status,
            content_type: None,
            body: Vec::new(),
        }
    }
}

struct MockServer {
    base_url: String,
    request: Receiver<Result<CapturedRequest, String>>,
    worker: JoinHandle<()>,
}

impl MockServer {
    fn start(response: MockResponse) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .expect("the one-shot mock server should bind to a local port");
        listener
            .set_nonblocking(true)
            .expect("the mock listener should become nonblocking");
        let address = listener
            .local_addr()
            .expect("the mock listener should have a local address");
        let (sender, request) = mpsc::channel();
        let worker = thread::spawn(move || {
            let outcome = serve_once(&listener, &response).map_err(|error| error.to_string());
            let _ = sender.send(outcome);
        });

        Self {
            base_url: format!("http://{address}"),
            request,
            worker,
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn finish(self) -> CapturedRequest {
        let request = self
            .request
            .recv_timeout(SERVER_TIMEOUT)
            .expect("the mock server should capture one request")
            .unwrap_or_else(|error| panic!("the mock server failed: {error}"));
        self.worker
            .join()
            .expect("the mock server worker should not panic");
        request
    }
}

#[derive(Debug)]
struct CapturedRequest {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl CapturedRequest {
    fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("request is missing the {name} header"))
    }
}

fn serve_once(listener: &TcpListener, response: &MockResponse) -> io::Result<CapturedRequest> {
    let mut stream = accept_with_timeout(listener)?;
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(SERVER_TIMEOUT))?;
    stream.set_write_timeout(Some(SERVER_TIMEOUT))?;

    let request = read_request(&mut stream)?;
    let mut headers = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body.len()
    );
    if let Some(content_type) = response.content_type {
        headers.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    headers.push_str("\r\n");

    stream.write_all(headers.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(request)
}

fn accept_with_timeout(listener: &TcpListener) -> io::Result<TcpStream> {
    let deadline = Instant::now() + SERVER_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for a mock-server connection",
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> io::Result<CapturedRequest> {
    const HEADER_END: &[u8] = b"\r\n\r\n";
    const MAX_REQUEST_SIZE: usize = 1024 * 1024;

    let mut bytes = Vec::new();
    let mut expected_size = None;
    loop {
        let mut buffer = [0_u8; 4096];
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > MAX_REQUEST_SIZE {
            return Err(invalid_data("mock request exceeds one MiB"));
        }

        if expected_size.is_none()
            && let Some(header_end) = find_bytes(&bytes, HEADER_END)
        {
            let content_length = parse_content_length(&bytes[..header_end])?;
            expected_size = Some(header_end + HEADER_END.len() + content_length);
        }
        if expected_size.is_some_and(|size| bytes.len() >= size) {
            break;
        }
    }

    let header_end = find_bytes(&bytes, HEADER_END)
        .ok_or_else(|| invalid_data("mock request has no complete header block"))?;
    let head = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| invalid_data("mock request headers are not UTF-8"))?;
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| invalid_data("mock request has no request line"))?;
    let mut request_line = request_line.split_whitespace();
    let method = request_line
        .next()
        .ok_or_else(|| invalid_data("mock request has no method"))?
        .to_owned();
    let target = request_line
        .next()
        .ok_or_else(|| invalid_data("mock request has no target"))?
        .to_owned();

    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| invalid_data("mock request contains a malformed header"))?;
        headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
    }

    let body_start = header_end + HEADER_END.len();
    let content_length = parse_content_length(&bytes[..header_end])?;
    let body_end = body_start + content_length;
    if bytes.len() < body_end {
        return Err(invalid_data("mock request body is incomplete"));
    }

    Ok(CapturedRequest {
        method,
        target,
        headers,
        body: bytes[body_start..body_end].to_vec(),
    })
}

fn parse_content_length(headers: &[u8]) -> io::Result<usize> {
    let headers = std::str::from_utf8(headers)
        .map_err(|_| invalid_data("mock request headers are not UTF-8"))?;
    for line in headers.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value
                .trim()
                .parse()
                .map_err(|_| invalid_data("mock request has an invalid content length"));
        }
    }
    Ok(0)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
