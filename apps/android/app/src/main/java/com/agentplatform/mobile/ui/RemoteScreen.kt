package com.agentplatform.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.data.AgentGateway
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.GatewayException
import java.net.URLEncoder
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

private fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
private fun encoded(value: String): String = URLEncoder.encode(value, "UTF-8")
private fun remoteMessage(error: Exception): String = when ((error as? GatewayException)?.code) {
    "runner_offline" -> "电脑已离线，恢复连接后可继续。"
    "session_control_lost", "host_generation_mismatch" -> "会话连接已失效，请恢复会话后重试。"
    "approval_not_pending" -> "审批已处理或过期。"
    "session_busy_or_unknown" -> "会话仍在执行，或状态尚未确认。"
    else -> if ((error as? GatewayException)?.status == 401) "请在设置中重新登录。" else "暂时无法连接远程服务，请检查设置和电脑连接。"
}

@Composable
fun RemoteScreen(config: AppConfig, modifier: Modifier = Modifier) {
    val gateway = remember(config) { AgentGateway(config) }
    val scope = rememberCoroutineScope()
    var snapshot by remember { mutableStateOf(JSONObject()) }
    var selected by remember { mutableStateOf<Pair<String, String>?>(null) }
    var events by remember { mutableStateOf(listOf<JSONObject>()) }
    var commands by remember { mutableStateOf(listOf<JSONObject>()) }
    var draft by remember { mutableStateOf("") }
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var create by remember { mutableStateOf(false) }
    val attempts = remember(gateway) { mutableMapOf<String, JSONObject>() }

    LaunchedEffect(gateway) {
        while (true) {
            try {
                snapshot = withContext(Dispatchers.IO) { gateway.remoteRequest("/sessions") }
            } catch (cancel: CancellationException) { throw cancel
            } catch (failure: Exception) {
                snapshot = JSONObject(snapshot.toString()).also { stale ->
                    stale.optJSONArray("sessions")?.objects()?.forEach { it.put("online", false) }
                    stale.optJSONArray("runners")?.objects()?.forEach { it.put("online", false) }
                }
                error = remoteMessage(failure)
            }
            delay(1500)
        }
    }
    LaunchedEffect(gateway, selected) {
        events = emptyList()
        commands = emptyList()
        draft = ""
        val target = selected ?: return@LaunchedEffect
        var cursor = 0L
        while (true) {
            try {
                val batch = withContext(Dispatchers.IO) {
                    gateway.remoteRequest("/events?runner=${encoded(target.first)}&hub=${encoded(target.second)}&after=$cursor").getJSONArray("events").objects()
                }
                if (batch.isNotEmpty()) {
                    cursor = batch.last().getLong("cursor")
                    events = events + batch
                }
                commands = withContext(Dispatchers.IO) {
                    gateway.remoteRequest("/sessions/${encoded(target.first)}/${encoded(target.second)}").getJSONArray("commands").objects()
                }
            } catch (cancel: CancellationException) { throw cancel
            } catch (failure: Exception) { error = remoteMessage(failure) }
            delay(500)
        }
    }
    val sessions = snapshot.optJSONArray("sessions")?.objects().orEmpty()
    val session = sessions.find { it.optString("runner_id") == selected?.first && it.getJSONObject("binding").optString("hub_session_id") == selected?.second }
    val binding = session?.optJSONObject("binding")
    val connected = session?.optBoolean("online") == true && binding?.optBoolean("control_connected") == true
    val idle = connected && binding?.optString("execution_state") in listOf("idle", "completed")
    fun submit(runner: String, hub: String, action: JSONObject, generation: String? = null) {
        if (busy) return
        busy = true
        error = ""
        val key = "$runner:${if (action.optString("mode") == "create") "new" else hub}:$action"
        scope.launch {
            try {
                val command = attempts.getOrPut(key) { JSONObject().put("client_cmd_id", UUID.randomUUID().toString()).put("hub_session_id", hub)
                    .put("host_generation", generation ?: JSONObject.NULL).put("command", action)
                }
                withContext(Dispatchers.IO) { gateway.remoteRequest("/runners/${encoded(runner)}/commands", command) }
                attempts.remove(key)
                if (action.optString("action") == "send_prompt") draft = ""
                selected = runner to command.getString("hub_session_id")
                create = false
            } catch (cancel: CancellationException) { throw cancel
            } catch (failure: Exception) {
                if (failure is GatewayException && failure.status in 400..499) attempts.remove(key)
                error = remoteMessage(failure)
            } finally { busy = false }
        }
    }
    fun control(action: JSONObject) {
        selected?.let { submit(it.first, it.second, action, binding?.optString("host_generation")) }
    }
    BackHandler(selected != null) { selected = null }
    Column(modifier.fillMaxSize().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            TextButton(onClick = { selected = null }) { Text(if (selected == null) "远程会话" else "返回会话") }
            TextButton(onClick = { create = true }, enabled = !busy) { Text("新建") }
        }
        if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error)
        if (selected == null) {
            if (sessions.isEmpty()) Text("连接本机 Runner 后，在这里开始或接续工作。")
            LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                items(sessions) { item ->
                    val b = item.getJSONObject("binding")
                    OutlinedButton(onClick = { selected = item.getString("runner_id") to b.getString("hub_session_id") }, modifier = Modifier.fillMaxWidth()) {
                        Column { Text(b.getString("workspace_path").substringAfterLast('/').substringAfterLast('\\')); Text("Codex · ${b.getString("provider_session_id").take(12)} · ${if (item.optBoolean("online")) "在线" else "离线"}") }
                    }
                }
            }
        } else {
            Text(if (connected) "已连接 · ${binding?.optString("execution_state")}" else "连接待恢复 · 历史仍可查看")
            val conversation = remember(events, commands) { remoteConversation(events, commands) }
            LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                items(conversation) { Text(it) }
            }
            commands.lastOrNull()?.optJSONObject("receipt")?.let { receipt ->
                if (receipt.optString("status") in listOf("rejected", "indeterminate"))
                    Text("请求未确认成功，请检查本机 Agent 状态。", color = MaterialTheme.colorScheme.error)
            }
            remoteApprovals(events).forEach { (id, params) ->
                Text(params.optString("command").ifBlank { "文件更改审批" })
                Text(params.optString("reason"))
                Row {
                    listOf(true to "允许", false to "拒绝").forEach { (approved, label) ->
                        TextButton(enabled = connected && !busy, onClick = {
                            control(JSONObject().put("action", "resolve_approval").put("approval_id", id).put("approved", approved))
                        }) { Text(label) }
                    }
                }
            }
            if (!connected && binding != null && session?.optBoolean("online") == true) TextButton(enabled = !busy, onClick = {
                control(JSONObject().put("action", "connect").put("mode", "resume").put("workspace_path", binding.getString("workspace_path")).put("provider_session_id", binding.getString("provider_session_id")))
            }) { Text("恢复原生会话") }
            OutlinedTextField(draft, { draft = it }, modifier = Modifier.fillMaxWidth(), label = { Text("继续这个会话") }, enabled = idle && !busy)
            Row {
                Button(enabled = idle && !busy && draft.isNotBlank(), onClick = { control(JSONObject().put("action", "send_prompt").put("text", draft)) }) { Text("发送") }
                TextButton(enabled = connected && !idle && !busy, onClick = { control(JSONObject().put("action", "interrupt")) }) { Text("停止任务") }
            }
        }
    }
    if (create) AlertDialog(onDismissRequest = { create = false }, title = { Text("选择电脑与工作目录") }, text = {
        LazyColumn {
            snapshot.optJSONArray("runners")?.objects().orEmpty().filter { it.optBoolean("online") }.forEach { runner ->
                val paths = runner.optJSONObject("hello")?.optJSONArray("workspaces") ?: JSONArray()
                items(paths.length()) { index ->
                    val path = paths.getString(index)
                    TextButton(enabled = !busy, onClick = {
                        submit(runner.getString("runner_id"), UUID.randomUUID().toString(), JSONObject().put("action", "connect").put("mode", "create").put("workspace_path", path).put("sandbox", "read-only").put("approval_policy", "untrusted"))
                    }) { Text("${runner.getString("runner_id").takeLast(8)} · $path") }
                }
            }
        }
    }, confirmButton = { TextButton(onClick = { create = false }) { Text("关闭") } })
}

private fun remoteConversation(events: List<JSONObject>, commands: List<JSONObject>): List<String> {
    val result = linkedMapOf<String, String>()
    val prompts = commands.mapNotNull { command -> command.optJSONObject("receipt")?.optJSONObject("result")?.optJSONObject("turn")?.optString("id")?.let { it to command.getJSONObject("command").getJSONObject("command").optString("text") } }.toMap()
    for (envelope in events) {
        val event = envelope.optJSONObject("payload")?.optJSONObject("event") ?: continue
        val p = event.optJSONObject("raw")?.optJSONObject("params") ?: continue
        val item = p.optJSONObject("item")
        val id = p.optString("itemId").ifBlank { item?.optString("id").orEmpty() }
        when (event.optString("kind")) {
            "turn/started" -> p.optJSONObject("turn")?.optString("id")?.let { turn -> prompts[turn]?.let { result["user:$turn"] = "你：$it" } }
            "item/agentMessage/delta" -> if (id.isNotEmpty()) result[id] = result[id].orEmpty() + p.optString("delta")
            "item/completed" -> if (id.isNotEmpty()) when (item?.optString("type")) {
                "agentMessage" -> result[id] = item.optString("text")
                "commandExecution" -> result[id] = "${item.optString("command")}\n${item.optString("aggregatedOutput")}"
            }
        }
    }
    return result.values.toList()
}

private fun remoteApprovals(events: List<JSONObject>): Map<String, JSONObject> {
    val pending = linkedMapOf<String, JSONObject>()
    for (envelope in events) {
        val event = envelope.optJSONObject("payload")?.optJSONObject("event") ?: continue
        val params = event.optJSONObject("raw")?.optJSONObject("params") ?: JSONObject()
        when (event.optString("kind")) {
            "approval_requested" -> pending[event.getString("approval_id")] = params
            "turn/completed" -> pending.clear()
            "serverRequest/resolved" -> {
                val requestId = params.opt("requestId")
                val id = if (requestId is String) JSONObject.quote(requestId) else requestId?.toString() ?: "null"
                pending.keys.removeAll { it.endsWith(":$id") }
            }
        }
    }
    return pending
}
