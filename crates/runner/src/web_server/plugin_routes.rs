use super::*;

fn refresh_after_plugin_mutation() {
    crate::collectors::invalidate_session_caches_for_refresh();
    crate::model_catalog::invalidate_model_catalog_cache();
    crate::remote_host::force_local_store_scan();
    invalidate_dashboard_summary_cache();
    crate::live::publish();
}

fn send_local_plugins_ok(
    stream: &mut TcpStream,
    extra_key: Option<(&str, serde_json::Value)>,
) -> anyhow::Result<()> {
    let (plugins, registry_url) = crate::plugins::list_local_plugins_view_cached();
    let mut payload = json!({
        "ok": true,
        "registry_url": registry_url,
        "plugins": plugins,
        "error": serde_json::Value::Null,
    });
    if let (Some((k, v)), Some(obj)) = (extra_key, payload.as_object_mut()) {
        obj.insert(k.to_string(), v);
    }
    send_json_response(stream, 200, &payload)
}

fn send_bad_plugin_request(
    stream: &mut TcpStream,
    err: impl std::fmt::Display,
) -> anyhow::Result<()> {
    send_json_response(
        stream,
        400,
        &json!({ "ok": false, "error": err.to_string() }),
    )
}

pub(crate) fn handle_plugin_route(
    stream: &mut TcpStream,
    method: &str,
    path: &str,
    body: &[u8],
) -> anyhow::Result<bool> {
    if method == "GET"
        && let Some(raw_id) = path.strip_prefix("/api/plugins/icon/")
    {
        let plugin_id = raw_id.trim();
        if plugin_id.is_empty()
            || plugin_id.contains('/')
            || plugin_id.contains('\\')
            || plugin_id.contains("..")
        {
            send_json_response(stream, 400, &json!({ "error": "invalid_plugin_id" }))?;
            return Ok(true);
        }
        if let Some((bytes, content_type)) = crate::plugins::read_plugin_icon(plugin_id) {
            send_sandboxed_response(
                stream,
                200,
                "OK",
                content_type,
                &bytes,
                Some("public, max-age=300"),
            )?;
        } else {
            send_json_response(stream, 404, &json!({ "error": "plugin_icon_not_found" }))?;
        }
        return Ok(true);
    }

    match (method, path) {
        ("GET", "/api/plugins") => {
            send_local_plugins_ok(stream, None)?;
            Ok(true)
        }
        ("POST", "/api/plugins/catalog") => {
            let (plugins, registry_url, err) = crate::plugins::list_plugins_view(true);
            let ok = err.is_none();
            send_json_response(
                stream,
                200,
                &json!({
                    "ok": ok,
                    "refreshed": ok,
                    "registry_url": registry_url,
                    "plugins": plugins,
                    "error": err,
                }),
            )?;
            Ok(true)
        }
        ("POST", "/api/plugins/install") => {
            #[derive(Deserialize)]
            struct InstallReq {
                plugin_id: String,
                source_url: Option<String>,
                enable_after: Option<bool>,
            }
            let req = match serde_json::from_slice::<InstallReq>(body) {
                Ok(r) => r,
                Err(err) => {
                    send_bad_plugin_request(
                        stream,
                        format!("invalid_plugin_install_payload: {err}"),
                    )?;
                    return Ok(true);
                }
            };
            match crate::plugins::install_plugin(
                &req.plugin_id,
                req.source_url.as_deref(),
                req.enable_after.unwrap_or(true),
            ) {
                Ok(record) => {
                    refresh_after_plugin_mutation();
                    let val = serde_json::to_value(record).unwrap_or(serde_json::Value::Null);
                    send_local_plugins_ok(stream, Some(("plugin", val)))?;
                }
                Err(err) => send_bad_plugin_request(stream, err)?,
            }
            Ok(true)
        }
        ("POST", "/api/plugins/uninstall") => {
            #[derive(Deserialize)]
            struct UninstallReq {
                plugin_id: String,
            }
            let req = match serde_json::from_slice::<UninstallReq>(body) {
                Ok(r) => r,
                Err(err) => {
                    send_bad_plugin_request(
                        stream,
                        format!("invalid_plugin_uninstall_payload: {err}"),
                    )?;
                    return Ok(true);
                }
            };
            match crate::plugins::uninstall_plugin(&req.plugin_id) {
                Ok(()) => {
                    refresh_after_plugin_mutation();
                    send_local_plugins_ok(stream, None)?;
                }
                Err(err) => send_bad_plugin_request(stream, err)?,
            }
            Ok(true)
        }
        ("POST", "/api/plugins/toggle") => {
            #[derive(Deserialize)]
            struct ToggleReq {
                plugin_id: String,
                enabled: Option<bool>,
            }
            let req = match serde_json::from_slice::<ToggleReq>(body) {
                Ok(r) => r,
                Err(err) => {
                    send_bad_plugin_request(
                        stream,
                        format!("invalid_plugin_toggle_payload: {err}"),
                    )?;
                    return Ok(true);
                }
            };
            match crate::plugins::toggle_plugin(&req.plugin_id, req.enabled) {
                Ok(record) => {
                    refresh_after_plugin_mutation();
                    let val = serde_json::to_value(record).unwrap_or(serde_json::Value::Null);
                    send_local_plugins_ok(stream, Some(("plugin", val)))?;
                }
                Err(err) => send_bad_plugin_request(stream, err)?,
            }
            Ok(true)
        }
        ("POST", "/api/plugins/registry") => {
            #[derive(Deserialize)]
            struct RegistryReq {
                registry_url: Option<String>,
            }
            let req = match serde_json::from_slice::<RegistryReq>(body) {
                Ok(r) => r,
                Err(err) => {
                    send_bad_plugin_request(
                        stream,
                        format!("invalid_plugin_registry_payload: {err}"),
                    )?;
                    return Ok(true);
                }
            };
            let cleaned = req
                .registry_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            match crate::plugins::set_registry_url(cleaned) {
                Ok(_) => {
                    invalidate_dashboard_summary_cache();
                    crate::live::publish();
                    send_local_plugins_ok(stream, None)?;
                }
                Err(err) => send_bad_plugin_request(stream, err)?,
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
