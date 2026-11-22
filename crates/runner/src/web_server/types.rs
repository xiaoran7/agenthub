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

pub(crate) fn parse_path_and_query(full_path: &str) -> (&str, HashMap<String, String>) {
    let mut parts = full_path.splitn(2, '?');
    let path = parts.next().unwrap_or("/");
    let mut query = HashMap::new();
    if let Some(q) = parts.next() {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                query.insert(k.to_string(), v.to_string());
            } else if !pair.is_empty() {
                query.insert(pair.to_string(), String::new());
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
