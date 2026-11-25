package com.agentplatform.mobile.data

import java.net.URI
import org.json.JSONArray
import org.json.JSONObject

// =========================================================================
// Remote v3 wire model
//
// One typed Kotlin shape per contract object in
// `packages/contracts/src/remote.ts`. The Gateway allowlist is the authority;
// anything it may omit stays nullable so a Runner that reports less never
// crashes the phone.
// =========================================================================

/** Remote execution and handoff states reported by the Runner. */
enum class ExecutionState(val wire: String, val label: String) {
    UNKNOWN("unknown", "未知"),
    IDLE("idle", "空闲"),
    RUNNING("running", "运行中"),
    AWAITING_PERMISSION("awaiting_permission", "等待授权"),
    AWAITING_INPUT("awaiting_input", "等待输入"),
    RELEASING_CONTROL("releasing_control", "正在归还控制"),
    ERROR("error", "异常"),
    COMPLETED("completed", "已完成"),
    TERMINATED("terminated", "已终止");

    /** The Agent is blocked on the person, not on the machine. */
    val waiting: Boolean
        get() = this == AWAITING_PERMISSION || this == AWAITING_INPUT

    val finished: Boolean
        get() = this == COMPLETED || this == TERMINATED

    val busy: Boolean
        get() = this == RUNNING

    /** Nothing is in flight, so an interrupt would be a no-op. */
    val settled: Boolean
        get() = this == IDLE || this == COMPLETED || this == TERMINATED || this == UNKNOWN

    companion object {
        fun fromWire(value: String): ExecutionState =
            entries.firstOrNull { it.wire == value } ?: UNKNOWN
    }
}

private val codexThreadId = Regex("^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$", RegexOption.IGNORE_CASE)

fun isCodexThreadId(id: String): Boolean = codexThreadId.matches(id)

data class RemoteBinding(
    val hubSessionId: String,
    val providerSessionId: String,
    val workspacePath: String,
    val hostGeneration: String,
    val controlConnected: Boolean,
    val executionState: ExecutionState,
    val currentTurnId: String?,
    val title: String = "",
) {
    /**
     * A session this phone can actually drive right now.
     *
     * The host flag is a parameter rather than read from a snapshot because a
     * binding can still claim a live control channel for a runner the Gateway
     * has already reported offline; every caller must pass what it knows.
     */
    fun controllable(hostOnline: Boolean): Boolean =
        hostOnline && controlConnected

    companion object {
        fun parse(json: JSONObject): RemoteBinding =
            RemoteBinding(
                hubSessionId = json.optStringOrEmpty("hub_session_id"),
                providerSessionId = json.optStringOrEmpty("provider_session_id"),
                workspacePath = json.optStringOrEmpty("workspace_path"),
                hostGeneration = json.optStringOrEmpty("host_generation"),
                controlConnected = json.optBoolean("control_connected"),
                executionState = ExecutionState.fromWire(json.optNullableString("execution_state").orEmpty()),
                currentTurnId = json.optNullableString("current_turn_id"),
                title = json.optNullableString("title").orEmpty(),
            )
    }
}

data class RemoteSessionRow(
    val runnerId: String,
    val online: Boolean,
    val binding: RemoteBinding,
    val provider: String? = null,
) {
    val key: String
        get() = "$runnerId/${binding.hubSessionId}"

    companion object {
        fun parse(json: JSONObject): RemoteSessionRow? {
            val binding = json.optJSONObject("binding") ?: return null
            val runnerId = json.optNullableString("runner_id").orEmpty()
            if (runnerId.isBlank()) return null
            if (binding.optNullableString("hub_session_id").isNullOrBlank()) return null
            return RemoteSessionRow(
                runnerId = runnerId,
                online = json.optBoolean("online"),
                binding = RemoteBinding.parse(binding),
                provider = json.optNullableString("provider"),
            )
        }
    }
}

/**
 * A conversation the Host found on disk but does not control.
 *
 * Discovery is metadata only: the phone can show it and ask the Host to
 * resume it, never read its transcript without a resume. The token fields are
 * what the context ring falls back to for a session that has emitted no usage
 * frame yet — a freshly resumed conversation is exactly that case.
 */
data class DiscoveryCandidate(
    val providerSessionId: String,
    val workspacePath: String,
    val title: String?,
    val provider: String?,
    val model: String?,
    val totalTokens: Long?,
    val lastActivityAtMs: Long?,
    val resumable: Boolean,
    /** Context window the Host measured, never an estimate. */
    val contextWindowTokens: Long? = null,
    /** Tokens already spent in this conversation, as the Host counted them. */
    val contextUsedTokens: Long? = null,
    val promptTokens: Long? = null,
    val requestCount: Long? = null,
) {
    /** Engine this conversation belongs to; drives which models may be sent. */
    val engine: EngineKind
        get() = engineOfSession(providerSessionId, provider)

    /**
     * Capacity to size the ring against when no usage frame has arrived.
     *
     * A measured window wins; otherwise the model name is the only honest
     * source, and 0 means "unknown" rather than a fabricated denominator.
     */
    val inferredContextWindow: Long
        get() = contextWindowTokens?.takeIf { it > 0 } ?: inferModelContextWindow(model, provider)
}

data class PendingApproval(
    val approvalId: String,
    val hubSessionId: String,
    val hostGeneration: String,
    val turnId: String,
    val canApprove: Boolean,
    val canDecline: Boolean,
    val toolName: String?,
    val commandPreview: String?,
    val path: String?,
    val diff: String?,
    val description: String?,
    val riskLevel: String?,
) {
    /** What the card shows when every optional field is absent. */
    val summary: String
        get() = listOfNotNull(
            commandPreview?.takeIf { it.isNotBlank() },
            description?.takeIf { it.isNotBlank() },
            path?.takeIf { it.isNotBlank() },
        ).firstOrNull() ?: "Agent 请求执行受保护操作，请确认是否放行。"

    companion object {
        fun parse(json: JSONObject): PendingApproval =
            PendingApproval(
                approvalId = json.optStringOrEmpty("approval_id"),
                hubSessionId = json.optStringOrEmpty("hub_session_id"),
                hostGeneration = json.optStringOrEmpty("host_generation"),
                turnId = json.optStringOrEmpty("turn_id"),
                canApprove = json.optBoolean("can_approve", true),
                canDecline = json.optBoolean("can_decline", true),
                toolName = json.optNullableString("tool_name"),
                commandPreview = json.optNullableString("command_preview"),
                path = json.optNullableString("path"),
                diff = json.optNullableString("diff"),
                description = json.optNullableString("description"),
                riskLevel = json.optNullableString("risk_level"),
            )
    }
}

data class QuestionOption(
    val label: String,
    val description: String?,
    val preview: String?,
)

data class PendingQuestion(
    val questionId: String,
    val hubSessionId: String,
    val hostGeneration: String,
    val turnId: String,
    val prompt: String,
    val header: String?,
    val options: List<QuestionOption>,
    val multiSelect: Boolean,
) {
    companion object {
        fun parse(json: JSONObject): PendingQuestion =
            PendingQuestion(
                questionId = json.optStringOrEmpty("question_id"),
                hubSessionId = json.optStringOrEmpty("hub_session_id"),
                hostGeneration = json.optStringOrEmpty("host_generation"),
                turnId = json.optStringOrEmpty("turn_id"),
                prompt = json.optStringOrEmpty("prompt"),
                header = json.optNullableString("header"),
                options = json.optJSONArray("options").objects().map { option ->
                    QuestionOption(
                        label = option.optStringOrEmpty("label"),
                        description = option.optNullableString("description"),
                        preview = option.optNullableString("preview"),
                    )
                },
                multiSelect = json.optBoolean("multi_select"),
            )
    }
}

/** The `ready` frame a Host publishes, as the Gateway stores it in `hello`. */
data class HostHello(
    val hostGeneration: String,
    val deviceName: String?,
    val provider: String?,
    val workspaces: List<String>,
    val candidates: List<DiscoveryCandidate>,
    val workspaceLabels: Map<String, String> = emptyMap(),
    val models: ModelCatalog = ModelCatalog.Empty,
    val pendingApprovals: Int,
    val pendingQuestions: Int,
    val pendingApprovalsList: List<PendingApproval> = emptyList(),
    val pendingQuestionsList: List<PendingQuestion> = emptyList(),
    val plugins: List<AgentPlugin> = emptyList(),
) {
    companion object {
        val Empty = HostHello(
            hostGeneration = "",
            deviceName = null,
            provider = null,
            workspaces = emptyList(),
            candidates = emptyList(),
            workspaceLabels = emptyMap(),
            models = ModelCatalog.Empty,
            pendingApprovals = 0,
            pendingQuestions = 0,
        )

        fun parse(json: JSONObject?): HostHello? {
            if (json == null) return null
            val discovery = json.optJSONObject("discovery")
            val approvalsArr = json.optJSONArray("pending_approvals")
            val questionsArr = json.optJSONArray("pending_questions")
            val approvalsList = approvalsArr.objects().map(PendingApproval::parse)
            val questionsList = questionsArr.objects().map(PendingQuestion::parse)
            val pluginsList = json.optJSONArray("plugins").objects().mapNotNull(AgentPlugin::parse)
            return HostHello(
                hostGeneration = json.optStringOrEmpty("host_generation"),
                deviceName = json.optNullableString("device_name")?.trim()?.takeIf { it.isNotEmpty() },
                provider = json.optNullableString("provider"),
                workspaces = json.optJSONArray("workspaces")?.strings().orEmpty(),
                workspaceLabels = json.optStringMap("workspace_labels"),
                candidates = discovery?.optJSONArray("candidates").objects().mapNotNull { candidate ->
                    val providerSessionId = candidate.optStringOrEmpty("provider_session_id")
                    val workspace = candidate.optStringOrEmpty("workspace_path")
                    if (providerSessionId.isBlank() || workspace.isBlank()) return@mapNotNull null
                    DiscoveryCandidate(
                        providerSessionId = providerSessionId,
                        workspacePath = workspace,
                        title = candidate.optNullableString("title"),
                        provider = candidate.optNullableString("provider"),
                        model = candidate.optNullableString("model"),
                        totalTokens = candidate.optLongOrNull("total_tokens"),
                        lastActivityAtMs = candidate.optLongOrNull("last_activity_at_ms"),
                        resumable = candidate.optBoolean("resumable", true),
                        contextWindowTokens = candidate.optLongOrNull("context_window_tokens"),
                        contextUsedTokens = candidate.optLongOrNull("context_used_tokens"),
                        promptTokens = candidate.optLongOrNull("prompt_tokens"),
                        requestCount =
                            candidate.optLongOrNull("request_count")
                                ?: candidate.optLongOrNull("calls"),
                    )
                },
                models = ModelCatalog.parse(json.optJSONObject("models")),
                pendingApprovals = approvalsArr?.length() ?: 0,
                pendingQuestions = questionsArr?.length() ?: 0,
                pendingApprovalsList = approvalsList,
                pendingQuestionsList = questionsList,
                plugins = pluginsList,
            )
        }
    }
}

/**
 * Merges a projected `hello` from `GET /v3/remote/sessions` with the richer
 * `hello` cached from `GET /v3/remote/runners/:runner/hello`.
 *
 * `hostGeneration`, `deviceName`, `workspaces`, and `pendingApprovals*` MUST
 * always come from [fresh] so a Runner restart (`host_generation` rotation) or
 * resolved approval is reflected immediately; only `models` and `candidates`
 * (which the list endpoint strips) fall back to [cachedFull].
 */
fun mergeHostHello(fresh: HostHello?, cachedFull: HostHello?): HostHello? {
    if (fresh == null) return cachedFull
    if (cachedFull == null) return fresh
    return fresh.copy(
        workspaceLabels =
            if (fresh.workspaceLabels.isNotEmpty()) fresh.workspaceLabels
            else cachedFull.workspaceLabels,
        models = if (!fresh.models.isEmpty) fresh.models else cachedFull.models,
        candidates = if (fresh.candidates.isNotEmpty()) fresh.candidates else cachedFull.candidates,
        plugins = if (fresh.plugins.isNotEmpty()) fresh.plugins else cachedFull.plugins,
    )
}

data class PluginCapabilities(
    val discover: Boolean = true,
    val launch: Boolean = true,
    val resume: Boolean = true,
    val models: Boolean = true,
    val history: Boolean = true,
    val approvals: Boolean = false,
) {
    companion object {
        val Default = PluginCapabilities()
        val Empty = Default

        fun parse(json: JSONObject?): PluginCapabilities {
            if (json == null) return Default
            return PluginCapabilities(
                discover = json.optBoolean("discover", true),
                launch = json.optBoolean("launch", true),
                resume = json.optBoolean("resume", true),
                models = json.optBoolean("models", true),
                history = json.optBoolean("history", true),
                approvals = json.optBoolean("approvals", false),
            )
        }
    }
}

/** Extension plugin entry from `/v3/remote/plugins`. */
data class AgentPlugin(
    val id: String,
    val name: String,
    val displayName: String? = null,
    val version: String,
    val description: String,
    val category: String,
    val enabled: Boolean,
    val installed: Boolean = false,
    val installedVersion: String? = null,
    val available: Boolean = false,
    val runtimeStatus: String?,
    val error: String?,
    val sessionPrefix: String? = null,
    val sessionPrefixes: List<String> = emptyList(),
    val aliases: List<String> = emptyList(),
    val capabilities: PluginCapabilities = PluginCapabilities.Empty,
    val runnerId: String? = null,
) {
    val label: String
        get() = displayName?.takeIf { it.isNotBlank() } ?: name.ifBlank { id }
    val allSessionPrefixes: List<String>
        get() = (sessionPrefixes + listOfNotNull(sessionPrefix?.takeIf { it.isNotBlank() })).distinct()

    companion object {
        fun parse(json: JSONObject): AgentPlugin? {
            val id = json.optNullableString("id")?.trim().orEmpty()
            if (id.isEmpty()) return null
            val runtimeStatus =
                json.optNullableString("runtimeStatus")
                    ?: json.optNullableString("runtime_status")
            val installed =
                if (json.has("installed")) json.optBoolean("installed", false)
                else runtimeStatus != "not_installed"
            return AgentPlugin(
                id = id,
                name = json.optNullableString("name") ?: id,
                displayName = json.optNullableString("display_name") ?: json.optNullableString("displayName"),
                version = json.optNullableString("version") ?: "1.0.0",
                description = json.optNullableString("description").orEmpty(),
                category = json.optNullableString("category") ?: "agent",
                enabled = json.optBoolean("enabled", false),
                installed = installed,
                installedVersion = json.optNullableString("installed_version") ?: json.optNullableString("installedVersion"),
                available = json.optBoolean("available", false),
                runtimeStatus = runtimeStatus,
                error = json.optNullableString("error"),
                sessionPrefix = json.optNullableString("session_prefix") ?: json.optNullableString("sessionPrefix"),
                sessionPrefixes = (json.optJSONArray("sessionPrefixes") ?: json.optJSONArray("session_prefixes"))?.strings().orEmpty(),
                aliases = json.optJSONArray("aliases")?.strings().orEmpty(),
                capabilities = PluginCapabilities.parse(json.optJSONObject("capabilities")),
                runnerId = json.optNullableString("runner_id") ?: json.optNullableString("runner"),
            )
        }
    }
}

data class RunnerPluginSnapshot(
    val runnerId: String,
    val deviceName: String?,
    val online: Boolean,
    val registryUrl: String?,
    val updatedAt: String?,
    val plugins: List<AgentPlugin>,
) {
    companion object {
        fun parse(json: JSONObject): RunnerPluginSnapshot? {
            val runnerId = json.optStringOrEmpty("runner").trim().ifEmpty { json.optStringOrEmpty("runner_id").trim() }
            if (runnerId.isEmpty()) return null
            return RunnerPluginSnapshot(
                runnerId = runnerId,
                deviceName = json.optNullableString("device_name"),
                online = json.optBoolean("online", false),
                registryUrl = json.optNullableString("registry_url"),
                updatedAt = json.optNullableString("updated_at"),
                plugins = json.optJSONArray("plugins").objects().mapNotNull(AgentPlugin::parse),
            )
        }
    }
}

data class RemotePluginsResponse(
    val plugins: List<AgentPlugin>,
    val runners: List<RunnerPluginSnapshot> = emptyList(),
) {
    companion object {
        fun parse(json: JSONObject): RemotePluginsResponse {
            return RemotePluginsResponse(
                plugins = json.optJSONArray("plugins").objects().mapNotNull(AgentPlugin::parse),
                runners = json.optJSONArray("runners").objects().mapNotNull(RunnerPluginSnapshot::parse),
            )
        }
    }
}

/** One-time pairing token from `POST /v1/device-bindings`. */
data class DeviceBinding(
    val bindingCode: String,
    val expiresAt: String?,
) {
    companion object {
        fun parse(json: JSONObject): DeviceBinding? {
            val code = json.optStringOrEmpty("binding_code").trim()
            if (code.isEmpty()) return null
            return DeviceBinding(
                bindingCode = code,
                expiresAt = json.optNullableString("expires_at"),
            )
        }
    }
}

data class HostDevice(
    val runnerId: String,
    val online: Boolean,
    val revoked: Boolean,
    val hello: HostHello?,
    val codexSessionControl: Boolean = false,
) {
    val hostGeneration: String
        get() = hello?.hostGeneration.orEmpty()

    val workspaces: List<String>
        get() = hello?.workspaces.orEmpty()

    val workspaceLabels: Map<String, String>
        get() = hello?.workspaceLabels.orEmpty()

    /** Engines' model lists this Host advertised; empty for an older Runner. */
    val models: ModelCatalog
        get() = hello?.models ?: ModelCatalog.Empty

    companion object {
        fun parse(json: JSONObject): HostDevice? {
            val runnerId = json.optStringOrEmpty("runner_id")
            if (runnerId.isBlank()) return null
            return HostDevice(
                runnerId = runnerId,
                online = json.optBoolean("online"),
                revoked = json.optBoolean("revoked"),
                hello = HostHello.parse(json.optJSONObject("hello")),
                codexSessionControl = json.optBoolean("codex_session_control"),
            )
        }
    }
}

/** `GET /v3/remote/sessions`. */
data class Snapshot(
    val cursor: Long,
    val hosts: List<HostDevice>,
    val sessions: List<RemoteSessionRow>,
) {
    fun host(runnerId: String): HostDevice? = hosts.firstOrNull { it.runnerId == runnerId }

    fun session(runnerId: String, hubSessionId: String): RemoteSessionRow? =
        sessions.firstOrNull {
            it.runnerId == runnerId && it.binding.hubSessionId == hubSessionId
        }

    /**
     * The discovery record for one provider session, if the Host published it.
     *
     * This is the only source of a session's model name and measured context
     * window: the Gateway stores neither, and a session that has emitted no
     * usage frame yet would otherwise show nothing.
     */
    fun candidate(runnerId: String, providerSessionId: String): DiscoveryCandidate? =
        host(runnerId)?.hello?.candidates?.firstOrNull {
            it.providerSessionId == providerSessionId
        }

    val onlineHostCount: Int
        get() = hosts.count { it.online && !it.revoked }

    val waitingCount: Int
        get() = sessions.count { it.binding.executionState.waiting && it.online }

    val runningCount: Int
        get() = sessions.count { it.binding.executionState.busy && it.online }

    companion object {
        val Empty = Snapshot(0, emptyList(), emptyList())

        fun parse(json: JSONObject): Snapshot =
            Snapshot(
                cursor = json.optLong("cursor", 0),
                hosts = json.optJSONArray("runners").objects().mapNotNull(HostDevice::parse),
                sessions = json.optJSONArray("sessions").objects().mapNotNull(RemoteSessionRow::parse),
            )
    }
}

/**
 * One stored Gateway frame.
 *
 * `payload` stays a raw object on purpose: its inner shape is the Provider's
 * own event vocabulary, which the contract deliberately does not narrow. The
 * typed accessors below are what the rest of the app reads.
 */
data class RemoteEvent(
    val cursor: Long,
    val runnerId: String,
    val payload: JSONObject,
) {
    val payloadType: String
        get() = payload.optStringOrEmpty("type")

    /** The `event` object of a `type: "event"` frame. */
    val event: JSONObject?
        get() = if (payloadType == "event") payload.optJSONObject("event") else null

    val eventKind: String
        get() = event?.optStringOrEmpty("kind").orEmpty()

    val receipt: JSONObject?
        get() = if (payloadType == "receipt") payload.optJSONObject("receipt") else null

    companion object {
        fun parse(json: JSONObject): RemoteEvent =
            RemoteEvent(
                cursor = json.optLong("cursor", 0),
                runnerId = json.optStringOrEmpty("runner_id"),
                payload = json.optJSONObject("payload") ?: JSONObject(),
            )
    }
}

/**
 * A submitted command and the receipt the Host wrote back, if any.
 *
 * Equality is by JSON text, not by object identity: `org.json.JSONObject` does
 * not implement `equals`, so a data class here would report every re-parsed
 * command as changed and make the session screen rebuild its whole transcript
 * on every poll. The fingerprint is computed once, at parse time.
 */
class RemoteCommandRecord(
    val command: JSONObject,
    val receipt: JSONObject?,
) {
    private val fingerprint = command.toString() + "\u0000" + (receipt?.toString() ?: "")

    val action: String
        get() = command.optJSONObject("command")?.optStringOrEmpty("action").orEmpty()

    val hubSessionId: String
        get() = command.optStringOrEmpty("hub_session_id")

    override fun equals(other: Any?): Boolean =
        this === other || (other is RemoteCommandRecord && other.fingerprint == fingerprint)

    override fun hashCode(): Int = fingerprint.hashCode()

    override fun toString(): String = "RemoteCommandRecord($action)"

    companion object {
        fun parse(json: JSONObject): RemoteCommandRecord =
            RemoteCommandRecord(
                command = json.optJSONObject("command") ?: JSONObject(),
                receipt = json.optJSONObject("receipt"),
            )
    }
}

/**
 * `GET /v3/remote/sessions/:runner/:hub`.
 *
 * `resumeTranscript` / `resumeApprovals` point at the newest history snapshot
 * and the newest frame carrying pending approvals. A fresh reader starts after
 * whichever is earlier, so a long session is not replayed from cursor zero and
 * neither the history nor the approvals are lost.
 */
data class SessionDetail(
    val binding: RemoteBinding?,
    val cursor: Long,
    val resumeTranscript: Long?,
    val resumeApprovals: Long?,
    val commands: List<RemoteCommandRecord>,
) {
    /** Cursor a new reader should start after, or null to read everything. */
    val resumeAfter: Long?
        get() {
            val transcript = resumeTranscript
            val approvals = resumeApprovals
            return when {
                transcript == null && approvals == null -> null
                transcript == null -> approvals
                approvals == null -> transcript
                else -> minOf(transcript, approvals)
            }
        }

    companion object {
        fun parse(json: JSONObject): SessionDetail {
            val binding = json.optJSONObject("binding")
            val resume = json.optJSONObject("resume")
            return SessionDetail(
                binding = binding?.let(RemoteBinding::parse),
                cursor = json.optLong("cursor", 0),
                resumeTranscript = resume?.optLongOrNull("transcript"),
                resumeApprovals = resume?.optLongOrNull("approvals"),
                commands = json.optJSONArray("commands").objects().map(RemoteCommandRecord::parse),
            )
        }
    }
}

data class CommandReceipt(
    val status: String,
    val seq: Long,
    val clientCmdId: String,
)

/** `GET /v1/auth/me` plus the login response: only the fields the phone shows. */
data class AccountView(
    val userId: String,
    val email: String,
    val displayName: String,
    val workspaceName: String?,
    val plan: String?,
) {
    companion object {
        fun parse(json: JSONObject): AccountView {
            val user = json.optJSONObject("user") ?: JSONObject()
            val workspace = json.optJSONObject("workspace")
            val subscription = json.optJSONObject("workspace_subscription")
                ?: json.optJSONObject("subscription")
            return AccountView(
                userId = user.optNullableString("user_id").orEmpty(),
                email = user.optNullableString("email").orEmpty(),
                displayName = user.optNullableString("display_name") ?: user.optNullableString("email").orEmpty(),
                workspaceName = workspace?.optNullableString("name"),
                plan = subscription?.optNullableString("plan"),
            )
        }
    }
}

/**
 * Where this phone points and how it proves who it is.
 *
 * Two credential shapes are accepted because the Gateway accepts two: an
 * account session cookie (normal self-hosted deployment) and a bearer access
 * token (single-owner deployment with `AGENT_PLATFORM_ACCESS_TOKEN`). The
 * loopback development header is deliberately the last resort and never
 * leaves a local address.
 */
data class AppConfig(
    val gatewayBase: String,
    val accessToken: String = "",
    val developmentUserId: String = "",
    /** Raw `name=value` pair of the account session cookie. */
    val sessionCookie: String = "",
) {
    fun normalized(): AppConfig =
        copy(
            gatewayBase = gatewayBase.trim().trimEnd('/'),
            accessToken = accessToken.trim(),
            developmentUserId = developmentUserId.trim(),
            sessionCookie = sessionCookie.trim(),
        )

    val hasCredential: Boolean
        get() = normalized().let {
            it.accessToken.isNotBlank() || it.sessionCookie.isNotBlank()
        }

    /**
     * Everything wrong with the address alone, ignoring whether a credential
     * is attached yet.
     *
     * Sign-in has to run this check and not [validationError]: a first launch
     * has no cookie to show, and demanding one before the login request is
     * sent would make password sign-in impossible on any real deployment.
     */
    fun gatewayError(): String? {
        val normalized = normalized()
        val uri =
            runCatching { URI(normalized.gatewayBase) }.getOrNull()
                ?: return "Gateway 地址格式无效"
        val host = uri.host?.lowercase() ?: return "Gateway 地址缺少主机名"
        val loopback = isLoopbackUriHost(host)
        val scheme = uri.scheme?.lowercase()
        if (scheme != "https" && !(scheme == "http" && loopback)) {
            return "远程 Gateway 必须使用 HTTPS"
        }
        if (uri.userInfo != null || uri.query != null || uri.fragment != null) {
            return "Gateway 地址不能包含凭据、查询参数或片段"
        }
        return null
    }

    fun validationError(): String? {
        val normalized = normalized()
        gatewayError()?.let { return it }
        val host = runCatching { URI(normalized.gatewayBase) }.getOrNull()?.host?.lowercase().orEmpty()
        val loopback = isLoopbackUriHost(host)
        if (!loopback && !hasCredential) {
            return "生产 Gateway 必须登录账号或填写访问令牌"
        }
        if (loopback && !hasCredential && normalized.developmentUserId.isBlank()) {
            return "请登录账号或填写访问令牌；本机开发模式可填写开发用户"
        }
        if (normalized.accessToken.any { it.isISOControl() }) {
            return "访问令牌包含非法控制字符"
        }
        if (normalized.sessionCookie.any { it.isISOControl() || it == '\n' }) {
            return "登录凭据格式无效"
        }
        if (normalized.developmentUserId.length > 128) {
            return "开发用户标识过长"
        }
        if (normalized.developmentUserId.any { it.isISOControl() }) {
            return "开发用户标识包含非法控制字符"
        }
        return null
    }

    companion object {
        internal fun isLoopbackUriHost(rawHost: String?): Boolean {
            val host = rawHost?.lowercase()?.trim() ?: return false
            return host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "[::1]"
        }

        val LocalDevelopment =
            AppConfig(
                gatewayBase = "http://127.0.0.1:3001",
                accessToken = "",
                developmentUserId = "local-user",
            )
    }
}

// JSON helpers live in JsonHelpers.kt
