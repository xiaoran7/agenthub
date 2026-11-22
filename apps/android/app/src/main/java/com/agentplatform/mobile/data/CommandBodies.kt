package com.agentplatform.mobile.data

import org.json.JSONObject

// =========================================================================
// Command bodies
//
// The Gateway enforces an exact allowlist on every command
// (`parseRemoteCommand` in `packages/contracts/src/remote.ts`): an unknown key
// or a value outside the allowed set is a 400 `invalid_field`, and the Host
// never sees the command at all. So the bodies are built here, once, from a
// small typed input, and everything else composes them.
//
// Two rules from the contract that are easy to get wrong and are therefore
// enforced here rather than at each call site:
//
//   * `connect` with `mode: "create"` carries `agent_type`; any other mode
//     carries `provider_session_id` and must not carry `agent_type`.
//   * a field is only *present* when it was actually chosen. Sending
//     `effort: "default"` is not the same as sending nothing: the Host would
//     apply a literal "default" instead of its own default.
// =========================================================================

/** Native Remote v3 connection modes. */
enum class ConnectMode(val wire: String) {
    CREATE("create"),
    RESUME("resume"),
    PREVIEW("preview"),
    TAKE_CONTROL("take_control"),
    ATTACH("attach"),
    ;

    /** A create is the only mode that names an engine instead of a session. */
    val creates: Boolean
        get() = this == CREATE
}

/**
 * `connect.sandbox` value set.
 *
 * Typed rather than a string literal so the three values the contract allows
 * (`packages/contracts/src/remote.ts` line 208) are enumerated in one place and
 * a typo is a compile error. The phone always sends `workspace-write`: the
 * Runner's own sandbox is what the workspace already grants, and offering a
 * "read-only" choice here would be a switch the Host has no way to honour.
 */
enum class SandboxMode(val wire: String, val label: String) {
    READ_ONLY("read-only", "只读"),
    WORKSPACE_WRITE("workspace-write", "允许修改工作区"),
    DANGER_FULL_ACCESS("danger-full-access", "完全访问"),
    ;

    companion object {
        fun fromWire(value: String): SandboxMode? = entries.firstOrNull { it.wire == value }
    }
}

/**
 * `connect.approval_policy` value set.
 *
 * Derived from [PermissionMode] rather than chosen: the two fields are not the
 * same thing and the Host needs both — the permission mode is the provider's
 * own switch, while `approval_policy` is what the Runner forwards as its
 * tool-approval stance. Leaving the latter at "untrusted" while asking for
 * "acceptEdits" would still stop on every write.
 */
enum class ApprovalPolicy(val wire: String) {
    UNTRUSTED("untrusted"),
    ON_REQUEST("on-request"),
    NEVER("never"),
    ;
}

enum class PermissionMode(val wire: String, val label: String, val hint: String) {
    DEFAULT("default", "默认", "每次改动都要我确认"),
    ACCEPT_EDITS("acceptEdits", "自动编辑", "文件改动自动放行，命令仍确认"),
    PLAN("plan", "只读规划", "只出方案不动手，确认后再执行"),
    BYPASS("bypassPermissions", "完全自动", "所有操作自动放行，风险自担"),
    ;

    /**
     * The `approval_policy` this mode maps onto.
     *
     * The two fields are not the same thing and the Host needs both: the
     * permission mode is the provider's own switch, while `approval_policy`
     * is what the Runner forwards as its tool-approval stance. Leaving the
     * latter at "untrusted" while asking for "acceptEdits" would still stop
     * on every write.
     */
    val approvalPolicy: ApprovalPolicy
        get() =
            when (this) {
                DEFAULT -> ApprovalPolicy.UNTRUSTED
                ACCEPT_EDITS -> ApprovalPolicy.ON_REQUEST
                PLAN -> ApprovalPolicy.UNTRUSTED
                BYPASS -> ApprovalPolicy.NEVER
            }

    companion object {
        fun fromWire(value: String): PermissionMode? = entries.firstOrNull { it.wire == value }
    }
}

/** Per-turn reasoning effort; `default` means "let the Host decide". */
enum class EffortLevel(val wire: String, val label: String) {
    DEFAULT("default", "默认"),
    LOW("low", "低"),
    MEDIUM("medium", "中"),
    HIGH("high", "高"),
    MAX("max", "最大"),
    ;

    companion object {
        fun fromWire(value: String): EffortLevel? = entries.firstOrNull { it.wire == value }
    }
}

/** One file the person attached to a prompt. */
data class RemoteAttachment(
    val name: String,
    val mediaType: String,
    val data: String,
    val encoding: String,
) {
    /**
     * Guard rails the contract imposes and the picker cannot see.
     *
     * `text(att.data, 8 * 1024 * 1024)` is a *character* limit, and base64 is
     * roughly 4/3 the size of the bytes it encodes, so a 6MB file already
     * encodes past it. The picker therefore refuses anything over 4MB rather
     * than letting the Gateway reject the whole prompt.
     */
    fun fitsLimit(): Boolean =
        name.isNotBlank() && name.length <= 256 && mediaType.length <= 128 && data.length <= MAX_DATA_CHARS

    /** True on-disk byte count, reversing Base64 4/3 expansion for binary files. */
    val estimatedByteLength: Long
        get() =
            if (encoding == "base64") {
                val pad = data.takeLast(2).count { it == '=' }
                ((data.length.toLong() * 3L) / 4L - pad).coerceAtLeast(0L)
            } else {
                data.toByteArray(Charsets.UTF_8).size.toLong()
            }

    companion object {
        /** `text(att.data, 8 * 1024 * 1024)`, with headroom for base64 growth. */
        const val MAX_DATA_CHARS = 6 * 1024 * 1024

        /** `command.attachments.length` must be within 1..16. */
        const val MAX_COUNT = 16
    }
}

/**
 * Builds a `connect` body.
 *
 * `model` is sent only when the caller picked one; a blank string would make
 * the Host select a model literally named "".
 */
fun connectCommand(
    mode: ConnectMode,
    workspacePath: String,
    providerSessionId: String? = null,
    agentType: EngineKind? = null,
    model: String? = null,
    permissionMode: PermissionMode = PermissionMode.DEFAULT,
    sandbox: SandboxMode = SandboxMode.WORKSPACE_WRITE,
    agentTypeWire: String? = null,
): JSONObject {
    val body = JSONObject()
    body.put("action", "connect")
    body.put("mode", mode.wire)
    body.put("workspace_path", workspacePath)
    if (mode.creates) {
        val wire = agentTypeWire?.trim()?.takeIf { it.isNotEmpty() } ?: agentType?.wire
        if (wire != null) body.put("agent_type", wire)
    } else {
        val id = providerSessionId?.trim().orEmpty()
        // `parseRemoteCommand` calls `text(command.provider_session_id, 256)`
        // for every non-create mode, so a blank id is a 400 before it reaches
        // the Host. The caller checks this; the guard keeps the invariant.
        require(id.isNotEmpty()) { "a non-create connect needs a provider session id" }
        body.put("provider_session_id", id)
    }
    body.put("sandbox", sandbox.wire)
    body.put("approval_policy", permissionMode.approvalPolicy.wire)
    // Sent even when it is "default": the field is a *choice*, and a Host that
    // has its own default still needs to be told which one this phone means.
    body.put("permission_mode", permissionMode.wire)
    val chosen = model?.trim().orEmpty()
    if (chosen.isNotEmpty()) body.put("model", chosen)
    return body
}

/**
 * Builds a `send_prompt` body.
 *
 * `effort`, `model` and `permission_mode` are omitted when left at their
 * neutral value, so a plain prompt looks exactly like the one v2.6.2 sends and
 * an older Host is not asked to interpret a field it does not know.
 */
fun sendPromptCommand(
    text: String,
    effort: EffortLevel = EffortLevel.DEFAULT,
    model: String? = null,
    permissionMode: PermissionMode = PermissionMode.DEFAULT,
    attachments: List<RemoteAttachment> = emptyList(),
    includePermissionMode: Boolean = false,
    acquireControl: Boolean = false,
): JSONObject {
    val body = JSONObject()
    body.put("action", "send_prompt")
    if (acquireControl) body.put("acquire_control", true)
    val trimmed = text.trim()
    val usable = attachments.filter { it.fitsLimit() }
    if (trimmed.isEmpty() && usable.isEmpty()) {
        throw IllegalArgumentException("a prompt needs text or an attachment")
    }
    // The contract requires a non-blank `text`; a prompt that is only
    // attachments still needs a sentence, and saying what arrived is more
    // useful to the agent than an empty string.
    val promptText =
        if (trimmed.isNotEmpty()) {
            trimmed
        } else {
            "请查看附件：" + usable.joinToString("、") { it.name }
        }
    body.put("text", promptText)
    if (effort != EffortLevel.DEFAULT) body.put("effort", effort.wire)
    val chosen = model?.trim().orEmpty()
    if (chosen.isNotEmpty()) body.put("model", chosen)
    if (includePermissionMode || permissionMode != PermissionMode.DEFAULT) {
        body.put("permission_mode", permissionMode.wire)
    }
    if (usable.isNotEmpty()) {
        val array = org.json.JSONArray()
        // The contract allows 1..16; sending more is a 400 for the whole prompt,
        // and the picker's own cap cannot be the only guard — a caller that
        // builds a list programmatically would take the session down with it.
        usable.take(RemoteAttachment.MAX_COUNT).forEach { attachment ->
            array.put(
                JSONObject()
                    .put("name", attachment.name)
                    .put("media_type", attachment.mediaType)
                    .put("data", attachment.data)
                    .put("encoding", attachment.encoding),
            )
        }
        body.put("attachments", array)
    }
    return body
}
