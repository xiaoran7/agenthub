use super::*;

pub struct WebServerConfig {
    pub port: u16,
    pub open_browser: bool,
    /// The identity file the device name lives beside.
    ///
    /// The rename endpoint has to write the same file the Runner reads, so the
    /// path is carried in rather than assumed to be the default one.
    pub identity_path: PathBuf,
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            port: 4200,
            open_browser: true,
            identity_path: crate::device_name::default_identity_path(),
        }
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn parse_path_and_query(full_path: &str) -> (&str, HashMap<String, String>) {
    let mut parts = full_path.splitn(2, '?');
    let path = parts.next().unwrap_or("/");
    let mut query = HashMap::new();
    if let Some(q) = parts.next() {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                query.insert(percent_decode(k), percent_decode(v));
            } else if !pair.is_empty() {
                query.insert(percent_decode(pair), String::new());
            }
        }
    }
    (path, query)
}

/// One REST call as it arrived: the socket to answer on, what was asked, and
/// the daemon and port that answer it.
pub(crate) struct ApiRequest<'a> {
    pub(crate) stream: &'a mut TcpStream,
    pub(crate) method: &'a str,
    pub(crate) path: &'a str,
    pub(crate) query: &'a HashMap<String, String>,
    pub(crate) headers: &'a HashMap<String, String>,
    pub(crate) body: &'a [u8],
    pub(crate) handler: Arc<dyn DaemonRequestHandler>,
    pub(crate) stopped: Arc<AtomicBool>,
    pub(crate) port: u16,
    /// The identity file the device name lives beside.
    pub(crate) identity_path: PathBuf,
}
