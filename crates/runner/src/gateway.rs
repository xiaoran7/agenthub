use std::time::Duration;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use ureq::tls::{Certificate, RootCerts, TlsConfig};
use url::{Host, Url};

use crate::{
    device::DeviceIdentity,
    outbox::Outbox,
    sync::{SyncAck, SyncBatch, SyncError, apply_sync_ack, prepare_sync_batch},
};

const MAX_BINDING_CODE_LENGTH: usize = 128;
const MAX_CA_CERTIFICATE_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("gateway URL is invalid")]
    InvalidUrl,
    #[error("gateway URL must use HTTPS (HTTP is allowed only for loopback development)")]
    InsecureUrl,
    #[error("binding code is invalid")]
    InvalidBindingCode,
    #[error("gateway CA certificate is invalid")]
    InvalidCaCertificate,
    #[error("maximum sync batch count must be greater than zero")]
    InvalidBatchCount,
    #[error("gateway request timed out")]
    Timeout,
    #[error("gateway is unreachable")]
    Unreachable,
    #[error("gateway TLS connection failed")]
    TlsFailed,
    #[error("gateway rejected the request with HTTP status {status}")]
    Rejected { status: u16 },
    #[error("gateway returned an invalid response")]
    InvalidResponse,
    #[error("gateway response belongs to another device")]
    DeviceMismatch,
    #[error(transparent)]
    Sync(#[from] SyncError),
}

impl GatewayError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidUrl => "gateway_invalid_url",
            Self::InsecureUrl => "gateway_insecure_url",
            Self::InvalidBindingCode => "gateway_invalid_binding_code",
            Self::InvalidCaCertificate => "gateway_invalid_ca_certificate",
            Self::InvalidBatchCount => "gateway_invalid_batch_count",
            Self::Timeout => "gateway_timeout",
            Self::Unreachable => "gateway_unreachable",
            Self::TlsFailed => "gateway_tls_failed",
            Self::Rejected { .. } => "gateway_rejected",
            Self::InvalidResponse => "gateway_invalid_response",
            Self::DeviceMismatch => "gateway_device_mismatch",
            Self::Sync(_) => "gateway_sync_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceBinding {
    pub device_id: String,
    pub registered: bool,
}

#[derive(Debug, Deserialize)]
struct DeviceBindingResponse {
    device_id: String,
    registered: bool,
}

#[derive(Serialize)]
struct DeviceRegistration<'a> {
    binding_code: &'a str,
    device_id: &'a str,
    public_key: String,
    signature: String,
}

/// Signing input for the registration proof of possession: the same fields in
/// declaration order, without the signature, as compact JSON. The gateway
/// rebuilds these bytes to verify the caller holds the device private key.
#[derive(Serialize)]
struct UnsignedDeviceRegistration<'a> {
    binding_code: &'a str,
    device_id: &'a str,
    public_key: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PushSummary {
    pub batches_sent: usize,
    pub acknowledged: usize,
    pub pending_events: u64,
}

pub struct GatewayClient {
    base_url: String,
    agent: ureq::Agent,
}

impl GatewayClient {
    pub fn new(base_url: &str, timeout: Duration) -> Result<Self, GatewayError> {
        Self::new_with_ca_certificate(base_url, timeout, None)
    }

    pub fn new_with_ca_certificate(
        base_url: &str,
        timeout: Duration,
        ca_certificate_pem: Option<&[u8]>,
    ) -> Result<Self, GatewayError> {
        let base_url = validate_base_url(base_url)?;
        let mut config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(concat!("agent-platform-runner/", env!("CARGO_PKG_VERSION")))
            // Following a redirect could resend the request to an address the
            // HTTPS/loopback policy never validated.
            .max_redirects(0);
        if let Some(pem) = ca_certificate_pem {
            if pem.is_empty() || pem.len() > MAX_CA_CERTIFICATE_BYTES {
                return Err(GatewayError::InvalidCaCertificate);
            }
            let certificate =
                Certificate::from_pem(pem).map_err(|_| GatewayError::InvalidCaCertificate)?;
            config = config.tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::new_with_certs(&[certificate]))
                    .build(),
            );
        }
        let config = config.build();
        Ok(Self {
            base_url,
            agent: config.new_agent(),
        })
    }

    pub fn bind(
        &self,
        identity: &DeviceIdentity,
        binding_code: &str,
    ) -> Result<DeviceBinding, GatewayError> {
        if binding_code.is_empty()
            || binding_code.len() > MAX_BINDING_CODE_LENGTH
            || binding_code.chars().any(char::is_control)
        {
            return Err(GatewayError::InvalidBindingCode);
        }
        let public_key = identity.public_key_hex();
        let unsigned = UnsignedDeviceRegistration {
            binding_code,
            device_id: identity.device_id(),
            public_key: &public_key,
        };
        let signing_bytes =
            serde_json::to_vec(&unsigned).map_err(|_| GatewayError::InvalidBindingCode)?;
        let registration = DeviceRegistration {
            binding_code,
            device_id: identity.device_id(),
            signature: hex::encode(identity.sign(&signing_bytes).to_bytes()),
            public_key,
        };
        let response: DeviceBindingResponse =
            self.post_json("/v1/devices/register", &registration)?;
        if response.device_id != identity.device_id() {
            return Err(GatewayError::DeviceMismatch);
        }
        Ok(DeviceBinding {
            device_id: response.device_id,
            registered: response.registered,
        })
    }

    pub fn upload(&self, batch: &SyncBatch) -> Result<SyncAck, GatewayError> {
        self.post_json("/v1/sync/batches", batch)
    }

    fn post_json<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R, GatewayError> {
        let url = format!("{}{}", self.base_url, path);
        let mut response = self
            .agent
            .post(&url)
            .send_json(body)
            .map_err(classify_transport_error)?;
        response
            .body_mut()
            .read_json()
            .map_err(|_| GatewayError::InvalidResponse)
    }
}

pub fn push_outbox(
    client: &GatewayClient,
    outbox: &mut Outbox,
    identity: &DeviceIdentity,
    limit: usize,
    max_batches: usize,
) -> Result<PushSummary, GatewayError> {
    if max_batches == 0 {
        return Err(GatewayError::InvalidBatchCount);
    }

    let mut batches_sent = 0;
    let mut acknowledged = 0;
    while batches_sent < max_batches {
        let Some(batch) = prepare_sync_batch(outbox, identity, limit)? else {
            break;
        };
        let acknowledgement = client.upload(&batch)?;
        acknowledged += apply_sync_ack(outbox, &batch, &acknowledgement)?;
        batches_sent += 1;
    }

    Ok(PushSummary {
        batches_sent,
        acknowledged,
        pending_events: outbox.stats().map_err(SyncError::from)?.pending_events,
    })
}

fn validate_base_url(value: &str) -> Result<String, GatewayError> {
    let url = Url::parse(value).map_err(|_| GatewayError::InvalidUrl)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.cannot_be_a_base()
    {
        return Err(GatewayError::InvalidUrl);
    }
    if url.scheme() != "https" && !(url.scheme() == "http" && is_loopback(&url)) {
        return Err(GatewayError::InsecureUrl);
    }
    if url.host().is_none() {
        return Err(GatewayError::InvalidUrl);
    }

    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn classify_transport_error(error: ureq::Error) -> GatewayError {
    match error {
        ureq::Error::StatusCode(status) => GatewayError::Rejected { status },
        ureq::Error::Timeout(_) => GatewayError::Timeout,
        ureq::Error::Io(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            GatewayError::TlsFailed
        }
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_) => {
            GatewayError::Unreachable
        }
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => GatewayError::TlsFailed,
        ureq::Error::Json(_) => GatewayError::InvalidResponse,
        _ => GatewayError::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::{IpAddr, TcpListener},
        sync::Arc,
        thread::{self, JoinHandle},
    };

    use rcgen::generate_simple_self_signed;
    use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};

    use crate::domain::{
        ActivityStatus, AgentType, Confidence, ObservationMeta, ObservationSource, Presence,
        ProjectRef, SessionObservation, SessionState,
    };

    use super::*;

    #[test]
    fn accepts_https_and_loopback_http() {
        assert_eq!(
            validate_base_url("https://gateway.example/v1/tenant").unwrap(),
            "https://gateway.example/v1/tenant"
        );
        assert_eq!(
            validate_base_url("http://127.0.0.1:3000/").unwrap(),
            "http://127.0.0.1:3000"
        );
        assert!(validate_base_url("http://[::1]:3000").is_ok());
        assert!(validate_base_url("http://localhost:3000").is_ok());
    }

    #[test]
    fn rejects_insecure_remote_and_ambiguous_urls() {
        for value in [
            "http://gateway.example",
            "http://foo.localhost:3000",
            "http://localhost.:3000",
        ] {
            assert!(matches!(
                validate_base_url(value),
                Err(GatewayError::InsecureUrl)
            ));
        }
        for value in [
            "https://user:secret@gateway.example",
            "https://gateway.example?tenant=one",
            "https://gateway.example#fragment",
            "not a URL",
        ] {
            assert!(matches!(
                validate_base_url(value),
                Err(GatewayError::InvalidUrl)
            ));
        }
    }

    #[test]
    fn error_codes_are_stable_and_do_not_include_transport_details() {
        assert_eq!(GatewayError::Timeout.code(), "gateway_timeout");
        assert_eq!(
            GatewayError::Rejected { status: 403 }.code(),
            "gateway_rejected"
        );
        assert_eq!(
            GatewayError::InvalidResponse.code(),
            "gateway_invalid_response"
        );
        assert_eq!(
            GatewayError::InvalidCaCertificate.code(),
            "gateway_invalid_ca_certificate"
        );
        assert!(matches!(
            GatewayClient::new_with_ca_certificate(
                "https://gateway.example",
                Duration::from_secs(1),
                Some(b"not a certificate"),
            ),
            Err(GatewayError::InvalidCaCertificate)
        ));
    }

    #[test]
    fn recognizes_all_ip_loopback_addresses() {
        assert!(IpAddr::from([127, 12, 0, 1]).is_loopback());
        assert!(validate_base_url("http://127.12.0.1:3000").is_ok());
    }

    #[test]
    fn real_http_push_applies_ack_and_clears_outbox() {
        let identity = DeviceIdentity::generate();
        let expected_device_id = identity.device_id().to_owned();
        let (server, server_thread) = spawn_json_server(move |request| {
            assert_eq!(request["device_id"], expected_device_id);
            serde_json::json!({
                "protocol_version": request["protocol_version"],
                "batch_id": request["batch_id"],
                "device_id": request["device_id"],
                "accepted_through": request["last_sequence"]
            })
        });
        let client = GatewayClient::new(&server, Duration::from_secs(2)).unwrap();
        let mut outbox = Outbox::open_in_memory().unwrap();
        outbox.enqueue(&observation(&identity)).unwrap();

        let summary = push_outbox(&client, &mut outbox, &identity, 100, 1).unwrap();

        assert_eq!(
            summary,
            PushSummary {
                batches_sent: 1,
                acknowledged: 1,
                pending_events: 0,
            }
        );
        server_thread.join().unwrap();
    }

    #[test]
    fn private_ca_rejects_an_untrusted_connection_then_completes_real_tls() {
        let identity = DeviceIdentity::generate();
        let expected_device_id = identity.device_id().to_owned();
        let (server, ca_certificate, server_thread) = spawn_tls_json_server(move |request| {
            assert_eq!(request["device_id"], expected_device_id);
            serde_json::json!({
                "protocol_version": request["protocol_version"],
                "batch_id": request["batch_id"],
                "device_id": request["device_id"],
                "accepted_through": request["last_sequence"]
            })
        });
        let mut outbox = Outbox::open_in_memory().unwrap();
        outbox.enqueue(&observation(&identity)).unwrap();

        let untrusted = GatewayClient::new(&server, Duration::from_secs(2)).unwrap();
        let error = push_outbox(&untrusted, &mut outbox, &identity, 100, 1).unwrap_err();
        assert!(
            matches!(error, GatewayError::TlsFailed),
            "unexpected certificate failure classification: {error:?}"
        );
        assert_eq!(
            outbox.stats().unwrap().pending_events,
            1,
            "a certificate failure must not acknowledge the Outbox"
        );

        let trusted = GatewayClient::new_with_ca_certificate(
            &server,
            Duration::from_secs(2),
            Some(ca_certificate.as_bytes()),
        )
        .unwrap();
        let summary = push_outbox(&trusted, &mut outbox, &identity, 100, 1).unwrap();

        assert_eq!(summary.acknowledged, 1);
        assert_eq!(summary.pending_events, 0);
        server_thread.join().unwrap();
    }

    #[test]
    fn failed_upload_preserves_outbox_for_later_replay() {
        let identity = DeviceIdentity::generate();
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("runner.db");
        let mut outbox = Outbox::open(&database_path).unwrap();
        outbox.enqueue(&observation(&identity)).unwrap();
        let mut second = observation(&identity);
        second.agent_session_id = "session-after-disconnect".into();
        outbox.enqueue(&second).unwrap();

        // Reserve and release an ephemeral address so connecting to it fails
        // immediately without relying on an external network.
        let unavailable = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let unavailable_address = unavailable.local_addr().unwrap();
        drop(unavailable);
        let offline_client = GatewayClient::new(
            &format!("http://{unavailable_address}"),
            Duration::from_millis(250),
        )
        .unwrap();
        assert!(matches!(
            push_outbox(&offline_client, &mut outbox, &identity, 100, 1),
            Err(GatewayError::Unreachable | GatewayError::Timeout)
        ));
        assert_eq!(
            outbox.stats().unwrap().pending_events,
            2,
            "a transport failure must not acknowledge local events"
        );
        drop(outbox);

        // Opening the same SQLite database models a Runner process restart:
        // pending events and their sequence watermark must survive it.
        let mut outbox = Outbox::open(&database_path).unwrap();
        assert_eq!(outbox.stats().unwrap().pending_events, 2);
        let pending = outbox.pending(identity.device_id(), 10).unwrap();
        assert_eq!(
            pending
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );

        let expected_device_id = identity.device_id().to_owned();
        let (server, server_thread) = spawn_json_server(move |request| {
            assert_eq!(request["device_id"], expected_device_id);
            assert_eq!(request["first_sequence"], 1);
            assert_eq!(request["last_sequence"], 2);
            serde_json::json!({
                "protocol_version": request["protocol_version"],
                "batch_id": request["batch_id"],
                "device_id": request["device_id"],
                "accepted_through": request["last_sequence"]
            })
        });
        let online_client = GatewayClient::new(&server, Duration::from_secs(2)).unwrap();
        let summary = push_outbox(&online_client, &mut outbox, &identity, 100, 1).unwrap();

        assert_eq!(summary.acknowledged, 2);
        assert_eq!(summary.pending_events, 0);
        server_thread.join().unwrap();
        drop(outbox);
        assert_eq!(
            Outbox::open(&database_path)
                .unwrap()
                .stats()
                .unwrap()
                .pending_events,
            0,
            "the acknowledged state must also survive another restart"
        );
    }

    fn observation(identity: &DeviceIdentity) -> SessionObservation {
        SessionObservation::new(
            &ObservationMeta::now(identity.device_id()),
            AgentType::Codex,
            "session-http",
            ProjectRef {
                display_name: "safe-project".into(),
                fingerprint: "a".repeat(64),
            },
            SessionState {
                presence: Presence::Live,
                activity_status: ActivityStatus::Working,
                raw_status: "active".into(),
                raw_status_detail: BTreeMap::new(),
                waiting_reason: None,
            },
            ObservationSource::CodexAppServer,
            Confidence::Authoritative,
        )
    }

    fn spawn_json_server(
        handler: impl FnOnce(serde_json::Value) -> serde_json::Value + Send + 'static,
    ) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server_thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            serve_json_request(&mut stream, handler);
        });
        (format!("http://{address}"), server_thread)
    }

    fn spawn_tls_json_server(
        handler: impl FnOnce(serde_json::Value) -> serde_json::Value + Send + 'static,
    ) -> (String, String, JoinHandle<()>) {
        let certified = generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let certificate_pem = certified.cert.pem();
        let private_key = PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der());
        let config = Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![certified.cert.der().clone()], private_key.into())
                .unwrap(),
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server_thread = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut rejected =
                StreamOwned::new(ServerConnection::new(Arc::clone(&config)).unwrap(), socket);
            let mut probe = [0_u8; 1];
            assert!(
                rejected.read(&mut probe).is_err(),
                "the client without the private CA must reject the certificate"
            );

            let (socket, _) = listener.accept().unwrap();
            let mut trusted =
                StreamOwned::new(ServerConnection::new(Arc::clone(&config)).unwrap(), socket);
            serve_json_request(&mut trusted, handler);
        });
        (
            format!("https://127.0.0.1:{}", address.port()),
            certificate_pem,
            server_thread,
        )
    }

    fn serve_json_request(
        stream: &mut (impl Read + Write),
        handler: impl FnOnce(serde_json::Value) -> serde_json::Value,
    ) {
        let mut request = Vec::new();
        let body_start = loop {
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..body_start]).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        while request.len() < body_start + content_length {
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
        }
        let body: serde_json::Value =
            serde_json::from_slice(&request[body_start..body_start + content_length]).unwrap();
        let response = serde_json::to_vec(&handler(body)).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        )
        .unwrap();
        stream.write_all(&response).unwrap();
        stream.flush().unwrap();
    }
}
