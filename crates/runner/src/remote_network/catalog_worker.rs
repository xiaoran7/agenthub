use super::*;
use crate::remote_host::MultiProviderTransport;

const CATALOG_RPC_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_CATALOG_WAITERS: usize = 32;

pub(crate) struct CatalogDiscovery {
    pub(crate) raw: Value,
    pub(crate) workspaces: HashMap<String, PathBuf>,
}

pub(crate) struct CatalogSnapshot {
    pub(crate) models: Result<Value, String>,
    pub(crate) discovery: Result<CatalogDiscovery, String>,
}

pub(crate) enum CatalogQuery {
    Models,
    Discovery,
}

pub(crate) struct CatalogWorker {
    requests: Option<SyncSender<()>>,
    results: Receiver<CatalogSnapshot>,
    worker: Option<thread::JoinHandle<()>>,
    in_flight: bool,
    waiters: Vec<(CatalogQuery, SyncSender<Result<Value, String>>)>,
}

impl CatalogWorker {
    pub(crate) fn new(binary: Option<PathBuf>, workspaces: Vec<PathBuf>) -> Self {
        let mut host = None;
        Self::spawn(move || {
            let read = || -> Result<_, String> {
                let codex = binary
                    .as_ref()
                    .map(|path| CodexAppServer::start_catalog(path, CATALOG_RPC_TIMEOUT))
                    .transpose()
                    .map_err(|_| "catalog_provider_unavailable".to_owned())?;
                NativeSessionHost::new(MultiProviderTransport::new(codex), workspaces.clone())
                    .map_err(|error| error.code)
            };
            if host.is_none() {
                match read() {
                    Ok(created) => host = Some(created),
                    Err(code) => {
                        return CatalogSnapshot {
                            models: Err(code.clone()),
                            discovery: Err(code),
                        };
                    }
                }
            }
            let metadata = host.as_mut().expect("catalog host initialized");
            let models = metadata.models().map_err(|error| error.code);
            let discovered = metadata.discover();
            let reconnect = discovered
                .as_ref()
                .is_err_and(|error| error.outcome_unknown);
            let discovery = discovered
                .map(|raw| CatalogDiscovery {
                    raw,
                    workspaces: metadata.discovered_workspaces().clone(),
                })
                .map_err(|error| error.code);
            if reconnect {
                host = None;
            }
            CatalogSnapshot { models, discovery }
        })
    }

    fn spawn(mut refresh: impl FnMut() -> CatalogSnapshot + Send + 'static) -> Self {
        let (send, requests) = mpsc::sync_channel(1);
        let (results, receive) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            while requests.recv().is_ok() {
                if results.send(refresh()).is_err() {
                    break;
                }
            }
        });
        Self {
            requests: Some(send),
            results: receive,
            worker: Some(worker),
            in_flight: false,
            waiters: Vec::new(),
        }
    }

    pub(crate) fn request(&mut self) -> bool {
        if self.in_flight {
            return false;
        }
        self.in_flight = self
            .requests
            .as_ref()
            .is_some_and(|send| send.try_send(()).is_ok());
        self.in_flight
    }

    pub(crate) fn query(&mut self, kind: CatalogQuery, reply: SyncSender<Result<Value, String>>) {
        if self.waiters.len() >= MAX_CATALOG_WAITERS {
            let _ = reply.try_send(Err("catalog_queue_full".to_owned()));
        } else if self.in_flight || self.request() {
            self.waiters.push((kind, reply));
        } else {
            let _ = reply.try_send(Err("catalog_worker_stopped".to_owned()));
        }
    }

    pub(crate) fn take_result(&mut self) -> Option<CatalogSnapshot> {
        match self.results.try_recv() {
            Ok(snapshot) => {
                self.in_flight = false;
                Some(snapshot)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.in_flight = false;
                for (_, reply) in self.waiters.drain(..) {
                    let _ = reply.try_send(Err("catalog_worker_stopped".to_owned()));
                }
                None
            }
        }
    }

    pub(crate) fn reply(&mut self, snapshot: &CatalogSnapshot) {
        for (kind, reply) in self.waiters.drain(..) {
            let result = match kind {
                CatalogQuery::Models => snapshot.models.clone(),
                CatalogQuery::Discovery => snapshot
                    .discovery
                    .as_ref()
                    .map(|discovery| discovery.raw.clone())
                    .map_err(Clone::clone),
            };
            let _ = reply.try_send(result);
        }
    }
}

impl Drop for CatalogWorker {
    fn drop(&mut self) {
        self.requests.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl CatalogDiscovery {
    pub(crate) fn apply(&self, host: &mut NativeSessionHost<MultiProviderTransport>) {
        host.discovered_workspaces.clone_from(&self.workspaces);
        if let Some(candidates) = self.raw["candidates"].as_array() {
            for item in candidates {
                if let (Some(id), Some(cwd)) = (
                    item["provider_session_id"].as_str(),
                    item["workspace_path"].as_str(),
                ) {
                    host.transport.remember_discovered_session(
                        id,
                        cwd,
                        item["provider"].as_str(),
                        item["model"].as_str(),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_catalog_is_nonblocking_and_queries_share_the_same_refresh() {
        let (entered_send, entered) = mpsc::sync_channel(1);
        let (resume, release) = mpsc::sync_channel(1);
        let mut catalog = CatalogWorker::spawn(move || {
            entered_send.try_send(()).unwrap();
            release.recv_timeout(Duration::from_secs(2)).unwrap();
            CatalogSnapshot {
                models: Ok(json!({"data": [1]})),
                discovery: Ok(CatalogDiscovery {
                    raw: json!({"candidates": []}),
                    workspaces: HashMap::new(),
                }),
            }
        });
        assert!(catalog.request());
        entered.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!catalog.request());
        assert!(catalog.take_result().is_none());
        let (model_send, models) = mpsc::sync_channel(1);
        let (discovery_send, discovery) = mpsc::sync_channel(1);
        catalog.query(CatalogQuery::Models, model_send);
        catalog.query(CatalogQuery::Discovery, discovery_send);
        assert!(models.try_recv().is_err());
        assert!(discovery.try_recv().is_err());
        resume.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let result = loop {
            if let Some(result) = catalog.take_result() {
                break result;
            }
            assert!(Instant::now() < deadline, "catalog did not return");
            thread::sleep(Duration::from_millis(5));
        };
        catalog.reply(&result);
        assert_eq!(models.try_recv().unwrap().unwrap(), json!({"data": [1]}));
        assert_eq!(
            discovery.try_recv().unwrap().unwrap(),
            json!({"candidates": []})
        );
    }
}
