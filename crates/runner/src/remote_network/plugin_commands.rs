use super::*;
use crate::plugins::RemotePluginCommand;

pub(crate) struct PluginCommandTaskResult {
    pub(crate) epoch: u64,
    pub(crate) frame: Value,
}

pub(crate) fn build_plugin_state_frame(
    request_id: Option<&str>,
    registry_url: &str,
    plugins: &[crate::plugins::PluginView],
    ok: bool,
    error: Option<&str>,
) -> Value {
    json!({
        "type": "plugin_state",
        "request_id": request_id,
        "registry_url": registry_url,
        "plugins": plugins,
        "ok": ok,
        "error": error,
    })
}

pub(crate) fn spawn_plugin_command_task(
    epoch: u64,
    frame: &Value,
    send_tx: &SyncSender<NetworkSend>,
    result_tx: &mpsc::Sender<PluginCommandTaskResult>,
) {
    let cmd = match serde_json::from_value::<RemotePluginCommand>(frame.clone()) {
        Ok(cmd) => cmd,
        Err(err) => {
            let req_id = frame.get("request_id").and_then(Value::as_str);
            let (plugins, reg_url) = crate::plugins::list_local_plugins_view_cached();
            let msg = format!("invalid_plugin_command:{err}");
            let payload = build_plugin_state_frame(req_id, &reg_url, &plugins, false, Some(&msg));
            let _ = send_tx.try_send(NetworkSend::Control(epoch, payload));
            return;
        }
    };

    if cmd.action == "install"
        && let Some(pid) = cmd.plugin_id.as_deref()
    {
        let (views, reg_url) = crate::plugins::list_local_plugins_view_cached();
        let downloading = crate::plugins::downloading_plugin_state_frame(
            &views,
            pid,
            cmd.request_id.as_deref(),
            &reg_url,
        );
        let _ = send_tx.try_send(NetworkSend::Control(epoch, downloading));
    }

    let tx = result_tx.clone();
    thread::spawn(move || {
        let outcome = crate::plugins::handle_remote_plugin_command(&cmd);
        let payload = build_plugin_state_frame(
            cmd.request_id.as_deref(),
            &outcome.registry_url,
            &outcome.plugins,
            outcome.ok,
            outcome.error.as_deref(),
        );
        let _ = tx.send(PluginCommandTaskResult {
            epoch,
            frame: payload,
        });
    });
}
