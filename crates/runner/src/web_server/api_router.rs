use super::*;

impl ApiRequest<'_> {
    pub(crate) fn route(self) -> anyhow::Result<()> {
        let ApiRequest {
            stream,
            method,
            path,
            query,
            headers,
            body,
            handler,
            stopped,
            port,
            identity_path,
        } = self;
        let _ = body;
        match (method, path) {
            ("GET", "/api/dashboard") => {
                let force = query
                    .get("force")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
                if force {
                    invalidate_dashboard_summary_cache();
                    crate::collectors::invalidate_session_caches_for_refresh();
                    crate::remote_host::force_local_store_scan();
                    crate::usage_ledger::refresh_change();
                }
                let (body_bytes, etag) =
                    build_cached_dashboard_payload(&handler, port, &identity_path);
                if let Some(pending_nav) = crate::tray::take_pending_window_hash() {
                    if let Ok(mut data) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                        data["pending_nav"] = json!(pending_nav);
                        send_json_response(stream, 200, &data)?;
                    } else {
                        send_json_bytes_with_etag(stream, &etag, &body_bytes)?;
                    }
                } else if headers
                    .get("if-none-match")
                    .is_some_and(|v| v.trim() == etag)
                {
                    send_not_modified(stream, &etag)?;
                } else {
                    send_json_bytes_with_etag(stream, &etag, &body_bytes)?;
                }
            }
            // the usage page's per-request ledger: one record per model call,
            // aggregated into the trend, the rankings, the heatmap and the
            // request list. `period` is today | 7d | 30d | all.
            ("GET", "/api/usage/ledger") => {
                let period =
                    crate::usage_ledger::Period::parse(query.get("period").map(String::as_str));
                let limit = query
                    .get("limit")
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(50_000)
                    .clamp(1, 100_000);
                let force = query
                    .get("force")
                    .map(|v| v == "1" || v == "true")
                    .unwrap_or(false);
                let (generation, digest) =
                    crate::usage_ledger::digest_cached_with_force(period, limit, force);
                let etag = format!("\"ledger-{generation}-{}-{limit}\"", period.key());
                send_json_response_with_etag(stream, headers, &etag, digest.as_ref())?;
            }
            ("POST", "/api/usage/export-csv") => {
                #[derive(Deserialize)]
                struct ExportCsvReq {
                    filename: String,
                    csv: String,
                }
                match serde_json::from_slice::<ExportCsvReq>(body) {
                    Ok(req) => {
                        if req.csv.len() > 10 * 1024 * 1024 {
                            send_json_response(
                                stream,
                                413,
                                &json!({ "ok": false, "error": "csv_too_large" }),
                            )?;
                            return Ok(());
                        }
                        let base_name = Path::new(&req.filename)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default();
                        let safe_name: String = base_name
                            .chars()
                            .filter(|c| c.is_ascii_alphanumeric() || matches!(*c, '-' | '_' | '.'))
                            .collect();
                        let final_name = if !safe_name.starts_with('.')
                            && !safe_name.contains("..")
                            && safe_name.ends_with(".csv")
                            && safe_name.len() > 4
                        {
                            safe_name
                        } else {
                            "agenthub-usage.csv".to_string()
                        };
                        let home = std::env::var_os("USERPROFILE")
                            .or_else(|| std::env::var_os("HOME"))
                            .map(PathBuf::from)
                            .unwrap_or_else(|| PathBuf::from("."));
                        let downloads = home.join("Downloads");
                        let target_dir = if downloads.is_dir() {
                            downloads
                        } else {
                            let fallback = home.join(".agenthub").join("exports");
                            let _ = std::fs::create_dir_all(&fallback);
                            fallback
                        };
                        let full_path = target_dir.join(&final_name);
                        match std::fs::write(&full_path, req.csv.as_bytes()) {
                            Ok(()) => {
                                let resolved = dunce::canonicalize(&full_path).unwrap_or(full_path);
                                send_json_response(
                                    stream,
                                    200,
                                    &json!({
                                        "ok": true,
                                        "path": resolved.to_string_lossy(),
                                        "filename": final_name,
                                    }),
                                )?;
                            }
                            Err(err) => {
                                send_json_response(
                                    stream,
                                    500,
                                    &json!({
                                        "ok": false,
                                        "error": format!("write_csv_failed: {err}")
                                    }),
                                )?;
                            }
                        }
                    }
                    Err(err) => {
                        send_json_response(
                            stream,
                            400,
                            &json!({
                                "ok": false,
                                "error": format!("invalid_export_payload: {err}")
                            }),
                        )?;
                    }
                }
            }
            ("POST", "/api/reveal-path") => {
                #[derive(Deserialize)]
                struct RevealReq {
                    path: PathBuf,
                }
                match serde_json::from_slice::<RevealReq>(body) {
                    Ok(req) => {
                        let home = std::env::var_os("USERPROFILE")
                            .or_else(|| std::env::var_os("HOME"))
                            .map(PathBuf::from);
                        let allowed_roots: Vec<PathBuf> = home
                            .into_iter()
                            .flat_map(|h| {
                                [h.join("Downloads"), h.join(".agenthub").join("exports")]
                            })
                            .filter_map(|p| dunce::canonicalize(p).ok())
                            .collect();
                        let valid_csv = dunce::canonicalize(&req.path).ok().filter(|canon| {
                            canon.is_file()
                                && canon
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
                                && allowed_roots.iter().any(|root| canon.starts_with(root))
                        });
                        let Some(target) = valid_csv else {
                            send_json_response(
                                stream,
                                403,
                                &json!({ "ok": false, "error": "invalid_target_path" }),
                            )?;
                            return Ok(());
                        };
                        #[cfg(target_os = "windows")]
                        {
                            let _ = std::process::Command::new("explorer.exe")
                                .arg(format!("/select,{}", target.display()))
                                .spawn();
                        }
                        #[cfg(target_os = "macos")]
                        {
                            let _ = std::process::Command::new("open")
                                .arg("-R")
                                .arg(&target)
                                .spawn();
                        }
                        #[cfg(all(unix, not(target_os = "macos")))]
                        {
                            if let Some(parent) = target.parent() {
                                let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                            }
                        }
                        send_json_response(stream, 200, &json!({ "ok": true }))?;
                    }
                    Err(err) => {
                        send_json_response(
                            stream,
                            400,
                            &json!({
                                "ok": false,
                                "error": format!("invalid_reveal_payload: {err}")
                            }),
                        )?;
                    }
                }
            }
            // Deleting a session moves its files to the runner's trash: the
            // conversation's transcript, its subagents' folder and what
            // Coding Assistant keeps by its id. A session written to in the
            // last minute is refused, as its agent may still be running it.
            ("POST", "/api/sessions/delete") => {
                let ids = serde_json::from_slice::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("ids").and_then(|ids| ids.as_array().cloned()))
                    .unwrap_or_default();
                let mut deleted: Vec<String> = Vec::new();
                let mut refused: Vec<serde_json::Value> = Vec::new();
                for raw_id in ids.iter().filter_map(|id| id.as_str()).take(200) {
                    if let Some(ag_id) = raw_id.strip_prefix("antigravity_") {
                        match crate::collectors::delete_antigravity_session(ag_id) {
                            Ok(_trash) => deleted.push(raw_id.to_owned()),
                            Err(error) => refused.push(json!({
                                "id": raw_id,
                                "error": error.to_string(),
                            })),
                        }
                    } else if let Some(c_id) = raw_id.strip_prefix("claude_") {
                        match crate::collectors::delete_claude_session(c_id) {
                            Ok(_trash) => deleted.push(format!("claude_{c_id}")),
                            Err(error) => refused.push(json!({
                                "id": format!("claude_{c_id}"),
                                "error": error.to_string(),
                            })),
                        }
                    } else if let Some(cx_id) = raw_id.strip_prefix("codex_") {
                        match crate::collectors::delete_codex_session(cx_id) {
                            Ok(_trash) => deleted.push(raw_id.to_owned()),
                            Err(error) => refused.push(json!({
                                "id": raw_id,
                                "error": error.to_string(),
                            })),
                        }
                    } else {
                        // Bare session id: try Claude first (legacy bare id callers), then Codex UUID
                        match crate::collectors::delete_claude_session(raw_id) {
                            Ok(_trash) => deleted.push(format!("claude_{raw_id}")),
                            Err(c_err) => {
                                if c_err.to_string().contains("no such session") {
                                    match crate::collectors::delete_codex_session(raw_id) {
                                        Ok(_trash) => deleted.push(raw_id.to_owned()),
                                        Err(cx_err) => refused.push(json!({
                                            "id": raw_id,
                                            "error": cx_err.to_string(),
                                        })),
                                    }
                                } else {
                                    refused.push(json!({
                                        "id": format!("claude_{raw_id}"),
                                        "error": c_err.to_string(),
                                    }));
                                }
                            }
                        }
                    }
                }
                // the page refreshes the moment this answers, and both caches
                // it reads from would otherwise still list what just went to
                // the trash: a second delete of the same row would be told
                // "no such session" although the first one worked
                if !deleted.is_empty() {
                    invalidate_dashboard_summary_cache();
                    crate::usage_ledger::forget_transcripts();
                    // Every open console is told at once, not on the next
                    // pass: the row it still holds is the one just trashed.
                    crate::live::publish();
                }
                send_json_response(
                    stream,
                    200,
                    &json!({ "deleted": deleted, "refused": refused }),
                )?;
            }
            ("POST", "/api/shutdown") => {
                let resp = json!({
                    "status": "shutting_down",
                    "message": "AgentHub Runner 正在退出，托盘图标与服务已安全终止。"
                });
                send_json_response(stream, 200, &resp)?;
                stopped.store(true, Ordering::SeqCst);
                thread::spawn(|| {
                    thread::sleep(Duration::from_millis(250));
                    std::process::exit(0);
                });
            }
            ("POST", "/api/open-data-dir") => {
                crate::tray::open_data_dir();
                let resp = json!({
                    "status": "ok",
                    "message": "已在系统文件管理器中打开本地数据存储目录"
                });
                send_json_response(stream, 200, &resp)?;
            }
            // the window's caption is painted the page's colour by the theme
            // extension the desktop window opens with; Edge reads that theme as
            // it starts, so the page leaves its choice here for the next window
            ("POST", "/api/window-theme") => {
                let dark = serde_json::from_slice::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("dark").and_then(|d| d.as_bool()));
                let saved = dark.and_then(crate::tray::remember_window_theme);
                let resp = match (dark, saved) {
                    (None, _) => json!({ "ok": false, "error": "dark_must_be_a_boolean" }),
                    (Some(dark), Some(())) => json!({ "ok": true, "dark": dark }),
                    (Some(_), None) => json!({ "ok": false, "error": "home_dir_unavailable" }),
                };
                send_json_response(stream, 200, &resp)?;
            }
            ("GET", "/api/status") => {
                let resp = handler.handle(DaemonRequest::GetStatus);
                send_json_response(stream, 200, &resp)?;
            }
            ("GET", "/api/snapshot") => {
                let resp = handler.handle(DaemonRequest::GetSnapshot);
                send_json_response(stream, 200, &resp)?;
            }
            ("GET", "/api/events") => {
                let after_cursor = query
                    .get("after")
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or(0);
                let limit = query
                    .get("limit")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(100);
                let resp = handler.handle(DaemonRequest::GetEvents {
                    after_cursor,
                    limit,
                });
                send_json_response(stream, 200, &resp)?;
            }
            ("GET", "/api/models") => {
                let catalog_models = crate::model_catalog::fetch_catalog_models(false);
                if !catalog_models.is_empty() {
                    let resp = crate::model_catalog::catalog_models_to_json(&catalog_models);
                    send_json_response(stream, 200, &resp)?;
                } else {
                    let resp = handler.handle(DaemonRequest::GetModels);
                    send_json_response(stream, 200, &resp)?;
                }
            }
            ("POST", "/api/models/refresh") => {
                // Both caches answer this button: the live upstream catalog
                // and the gateway profile that reads the same endpoint.
                crate::claude_gateway::invalidate_claude_gateway_cache();
                crate::model_catalog::invalidate_model_catalog_cache();
                let catalog_models = crate::model_catalog::fetch_catalog_models(true);
                let resp = crate::model_catalog::catalog_models_to_json(&catalog_models);
                send_json_response(stream, 200, &resp)?;
            }
            ("GET", "/api/workspaces") => {
                let ws = crate::collectors::workspace::discover_all_workspaces();
                send_json_response(stream, 200, &json!({ "workspaces": ws }))?;
            }
            ("GET", "/api/capabilities") => {
                let caps = crate::collectors::capabilities::discover_capabilities(None);
                send_json_response(stream, 200, &json!({ "ok": true, "capabilities": caps }))?;
            }
            ("POST", "/api/capabilities/mcp/register") => {
                #[derive(Deserialize)]
                struct RegisterMcpReq {
                    name: String,
                    command: Option<String>,
                    #[serde(default)]
                    args: Vec<String>,
                    url: Option<String>,
                    #[serde(default)]
                    headers: HashMap<String, String>,
                }
                match serde_json::from_slice::<RegisterMcpReq>(body) {
                    Ok(req) => {
                        match crate::collectors::capabilities::register_agenthub_mcp_server(
                            &req.name,
                            req.command,
                            req.args,
                            req.url,
                            req.headers,
                        ) {
                            Ok(srv) => send_json_response(
                                stream,
                                200,
                                &json!({ "ok": true, "server": srv }),
                            )?,
                            Err(err) => send_json_response(
                                stream,
                                400,
                                &json!({ "ok": false, "error": err }),
                            )?,
                        }
                    }
                    Err(err) => {
                        let err_json =
                            json!({ "error": format!("invalid_mcp_register_payload: {}", err) });
                        send_json_response(stream, 400, &err_json)?;
                    }
                }
            }
            ("POST", "/api/capabilities/sync") => {
                // Invalidate dashboard summary and capability caches to ensure fresh view
                crate::collectors::capabilities::invalidate_capabilities_cache();
                invalidate_dashboard_summary_cache();
                crate::live::publish();
                let caps = crate::collectors::capabilities::discover_capabilities(None);
                let resp = json!({
                    "ok": true,
                    "message": "已完成智能体扩展生态 (Skills/MCP/Hooks) 探测与统一对齐",
                    "capabilities": caps
                });
                send_json_response(stream, 200, &resp)?;
            }
            ("GET", "/api/discover") => {
                let resp = handler.handle(DaemonRequest::Discover);
                send_json_response(stream, 200, &resp)?;
            }
            ("GET", "/api/conversation") => {
                let session_id = query.get("session_id").cloned().unwrap_or_default();
                let agent_str = query.get("agent").map(String::as_str).unwrap_or("claude");
                let normalized = agent_str.trim().to_ascii_lowercase();
                let agent_type = match normalized.as_str() {
                    "claude" | "claude_code" => AgentType::ClaudeCode,
                    "claude_desktop" => AgentType::ClaudeDesktop,
                    "codex" => AgentType::Codex,
                    "antigravity" => AgentType::Antigravity,
                    "deepseek" | "deepseek_harness" => AgentType::DeepseekHarness,
                    other => serde_json::from_value::<AgentType>(Value::String(other.to_string()))
                        .unwrap_or(AgentType::ClaudeCode),
                };
                let resp = handler.handle(DaemonRequest::GetSessionConversation {
                    agent_type,
                    session_id,
                });
                send_json_response(stream, 200, &resp)?;
            }
            ("POST", "/api/execute") => match serde_json::from_slice::<RemoteCommand>(body) {
                Ok(command) => {
                    let resp = handler.handle(DaemonRequest::Execute { command });
                    send_json_response(stream, 200, &resp)?;
                }
                Err(err) => {
                    let err_json = json!({ "error": format!("invalid_command_payload: {}", err) });
                    send_json_response(stream, 400, &err_json)?;
                }
            },
            ("POST", "/api/authorize-workspace") => {
                #[derive(Deserialize)]
                struct AuthWorkspaceReq {
                    workspace: PathBuf,
                }
                match serde_json::from_slice::<AuthWorkspaceReq>(body) {
                    Ok(req) => {
                        let resp = handler.handle(DaemonRequest::AuthorizeWorkspace {
                            workspace: req.workspace,
                        });
                        send_json_response(stream, 200, &resp)?;
                    }
                    Err(err) => {
                        let err_json =
                            json!({ "error": format!("invalid_workspace_payload: {}", err) });
                        send_json_response(stream, 400, &err_json)?;
                    }
                }
            }
            ("POST", "/api/device/bind") => {
                #[derive(Deserialize)]
                struct BindReq {
                    gateway: String,
                    binding_code: String,
                }
                match serde_json::from_slice::<BindReq>(body) {
                    Ok(req) => {
                        let home = std::env::var_os("USERPROFILE")
                            .or_else(|| std::env::var_os("HOME"))
                            .map(PathBuf::from);
                        let Some(home) = home else {
                            let err_json = json!({ "error": "home_dir_unavailable" });
                            send_json_response(stream, 500, &err_json)?;
                            return Ok(());
                        };
                        let identity = match crate::device::DeviceIdentity::load(&identity_path) {
                            Ok(id) => id,
                            Err(err) => {
                                let err_json = json!({
                                    "error": format!("load_identity_failed: {}", err),
                                    "message": format!("加载本机身份凭证失败：{}", err)
                                });
                                send_json_response(stream, 500, &err_json)?;
                                return Ok(());
                            }
                        };

                        let mut gateway_url = req.gateway.trim().trim_end_matches('/').to_string();
                        if !gateway_url.starts_with("http://")
                            && !gateway_url.starts_with("https://")
                        {
                            gateway_url = format!("https://{}", gateway_url);
                        }
                        let client = match crate::gateway::GatewayClient::new(
                            &gateway_url,
                            Duration::from_secs(10),
                        ) {
                            Ok(c) => c,
                            Err(err) => {
                                let err_json = json!({
                                    "error": format!("invalid_gateway_url: {}", err),
                                    "message": format!("网关地址不合法：{}", err)
                                });
                                send_json_response(stream, 400, &err_json)?;
                                return Ok(());
                            }
                        };

                        let dev_name = crate::device_name::resolve(None, Some(&identity_path));
                        let mut identity = identity;
                        let bind_res =
                            client.bind(&identity, req.binding_code.trim(), Some(&dev_name));
                        let bind_res = match bind_res {
                            Err(crate::gateway::GatewayError::Rejected {
                                error_code: Some(ref code),
                                ..
                            }) if code == "device_revoked" => {
                                eprintln!(
                                    "[agenthub-runner] Device identity was revoked by gateway; backing up old identity and regenerating..."
                                );
                                let new_identity = crate::device::DeviceIdentity::generate();
                                match new_identity.overwrite(&identity_path) {
                                    Ok(()) => {
                                        identity = new_identity;
                                        client.bind(
                                            &identity,
                                            req.binding_code.trim(),
                                            Some(&dev_name),
                                        )
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[agenthub-runner] Failed to overwrite revoked device identity: {}",
                                            e
                                        );
                                        Err(crate::gateway::GatewayError::Rejected {
                                            status: 403,
                                            error_code: Some("device_revoked".to_string()),
                                        })
                                    }
                                }
                            }
                            other => other,
                        };

                        match bind_res {
                            Ok(binding) => {
                                let binding_info = json!({
                                    "device_id": binding.device_id,
                                    "registered": binding.registered,
                                    "gateway": gateway_url,
                                    "paired_at": chrono::Utc::now().to_rfc3339()
                                });
                                let binding_path = home.join(".agenthub").join("binding.json");
                                let _ = std::fs::write(
                                    &binding_path,
                                    serde_json::to_string_pretty(&binding_info).unwrap_or_default(),
                                );
                                let _ = handler.handle(DaemonRequest::SetRemoteEnabled {
                                    server_url: Some(gateway_url),
                                });
                                invalidate_dashboard_summary_cache();
                                if let Ok(mut guard) = device_info_cache().write() {
                                    *guard = None;
                                }
                                let resp = json!({
                                    "ok": true,
                                    "message": "设备已成功完成配对与网关注册！",
                                    "binding": binding_info
                                });
                                send_json_response(stream, 200, &resp)?;
                            }
                            Err(err) => {
                                let friendly_message = match &err {
                                    crate::gateway::GatewayError::Rejected { status, error_code } => {
                                        match error_code.as_deref() {
                                            Some("invalid_binding_code") => {
                                                "配对码无效或已过期（有效期10分钟）。请在 Web 控制台重新生成最新的配对码。".to_string()
                                            }
                                            Some("binding_code_used") => {
                                                "该配对码已被使用，请在 Web 控制台生成新的配对码。".to_string()
                                            }
                                            Some("device_revoked") => {
                                                "当前设备已被网关撤销，自动重新生成身份凭据失败，请重试或重置身份。".to_string()
                                            }
                                            Some("device_not_registered") => {
                                                "设备尚未在网关注册，请确认配对流程。".to_string()
                                            }
                                            Some("device_key_mismatch") => {
                                                "设备公钥与网关记录不匹配。".to_string()
                                            }
                                            Some("device_not_owned") => {
                                                "该设备不属于当前用户账号。".to_string()
                                            }
                                            Some("device_conflict") => {
                                                "设备已被其他账号绑定或存在冲突。".to_string()
                                            }
                                            Some(code) => {
                                                format!("网关拒绝配对请求（{}，HTTP {}）", code, status)
                                            }
                                            None if *status == 403 => {
                                                "配对码无效或已被拒绝（HTTP 403）。请在 Web 控制台检查并重新生成配对码。".to_string()
                                            }
                                            None => {
                                                format!("网关拒绝绑定请求（HTTP 状态码 {}）", status)
                                            }
                                        }
                                    }
                                    crate::gateway::GatewayError::Timeout => {
                                        "连接网关超时，请检查网络或网关地址是否正确。".to_string()
                                    }
                                    crate::gateway::GatewayError::Unreachable => {
                                        "无法连接到网关，请检查地址是否正确或网关是否在线。".to_string()
                                    }
                                    crate::gateway::GatewayError::TlsFailed => {
                                        "与网关的 TLS/HTTPS 安全握手失败，请检查网关证书。".to_string()
                                    }
                                    crate::gateway::GatewayError::InvalidBindingCode => {
                                        "配对码格式无效（不能为空或含有非法字符）。".to_string()
                                    }
                                    crate::gateway::GatewayError::InvalidUrl | crate::gateway::GatewayError::InsecureUrl => {
                                        "网关地址格式不合法，请使用完整且规范的 https 地址。".to_string()
                                    }
                                    _ => format!("绑定失败：{}", err),
                                };
                                let err_json = json!({
                                    "error": format!("bind_failed: {}", err),
                                    "message": friendly_message
                                });
                                send_json_response(stream, 400, &err_json)?;
                            }
                        }
                    }
                    Err(err) => {
                        let err_json = json!({
                            "error": format!("invalid_bind_payload: {}", err),
                            "message": format!("请求参数解析失败：{}", err)
                        });
                        send_json_response(stream, 400, &err_json)?;
                    }
                }
            }
            ("POST", "/api/device/name") => {
                #[derive(Deserialize)]
                struct NameReq {
                    name: String,
                }
                match serde_json::from_slice::<NameReq>(body) {
                    Ok(req) => {
                        // Written beside the identity the Runner actually uses,
                        // so the running process picks the name up on its next
                        // publish instead of reading a different file.
                        match crate::device_name::write_configured_name(
                            &identity_path,
                            req.name.trim(),
                        ) {
                            Ok(()) => {
                                let name = crate::device_name::resolve(None, Some(&identity_path));
                                send_json_response(
                                    stream,
                                    200,
                                    &json!({ "ok": true, "device_name": name }),
                                )?;
                            }
                            Err(err) => {
                                let err_json =
                                    json!({ "error": format!("write_device_name_failed: {err}") });
                                send_json_response(stream, 500, &err_json)?;
                            }
                        }
                    }
                    Err(err) => {
                        let err_json = json!({ "error": format!("invalid_name_payload: {err}") });
                        send_json_response(stream, 400, &err_json)?;
                    }
                }
            }
            ("POST", "/api/device/unbind") => {
                let home = std::env::var_os("USERPROFILE")
                    .or_else(|| std::env::var_os("HOME"))
                    .map(PathBuf::from);
                if let Some(home) = home {
                    let binding_path = home.join(".agenthub").join("binding.json");
                    let _ = std::fs::remove_file(&binding_path);
                }
                let _ = handler.handle(DaemonRequest::SetRemoteEnabled { server_url: None });
                invalidate_dashboard_summary_cache();
                if let Ok(mut guard) = device_info_cache().write() {
                    *guard = None;
                }
                let resp = json!({
                    "ok": true,
                    "message": "已成功解除与网关的绑定关系"
                });
                send_json_response(stream, 200, &resp)?;
            }
            _ => {
                if !handle_plugin_route(stream, method, path, body)? {
                    let err_json = json!({ "error": "not_found" });
                    send_json_response(stream, 404, &err_json)?;
                }
            }
        }
        Ok(())
    }
}
