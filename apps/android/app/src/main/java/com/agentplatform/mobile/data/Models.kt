package com.agentplatform.mobile.data

import java.net.URI
import java.time.Instant

enum class AgentType(val wireValue: String, val label: String) {
    CLAUDE_CODE("claude_code", "Claude Code"),
    CODEX("codex", "Codex"),
    UNKNOWN("unknown", "未知 Agent");

    companion object {
        fun fromWire(value: String): AgentType =
            entries.firstOrNull { it.wireValue == value } ?: UNKNOWN
    }
}

enum class Presence {
    LIVE,
    PERSISTED,
    UNKNOWN;

    companion object {
        fun fromWire(value: String): Presence =
            entries.firstOrNull { it.name == value } ?: UNKNOWN
    }
}

enum class ActivityStatus(val label: String) {
    WORKING("工作中"),
    WAITING_INPUT("等待输入"),
    WAITING_PERMISSION("等待授权"),
    IDLE("空闲"),
    COMPLETED("已完成"),
    FAILED("失败"),
    STOPPED("已停止"),
    UNKNOWN("状态未知");

    val isWaiting: Boolean
        get() = this == WAITING_INPUT || this == WAITING_PERMISSION

    companion object {
        fun fromWire(value: String): ActivityStatus =
            entries.firstOrNull { it.name == value } ?: UNKNOWN
    }
}

data class ProjectInfo(
    val displayName: String,
    val pathFingerprint: String?,
)

data class SessionState(
    val presence: Presence,
    val activityStatus: ActivityStatus,
    val waitingReason: String?,
)

data class SessionRecord(
    val eventId: String,
    val sequence: Long,
    val deviceId: String,
    val agentType: AgentType,
    val agentSessionId: String,
    val observedAt: Instant,
    val receivedAt: Instant?,
    val project: ProjectInfo,
    val state: SessionState,
) {
    val key: String
        get() = "${deviceId}:${agentType.wireValue}:$agentSessionId"
}

data class DeviceRecord(
    val deviceId: String,
    val createdAt: Instant,
    val lastSeenAt: Instant,
    val online: Boolean,
    val revoked: Boolean,
)

data class GatewaySnapshot(
    val devices: List<DeviceRecord>,
    val sessions: List<SessionRecord>,
)

enum class SessionFilter(val label: String) {
    ALL("全部"),
    LIVE("进行中"),
    WAITING("待处理"),
    FINISHED("最近完成"),
    HISTORY("历史");
}

data class SessionCounts(
    val all: Int,
    val live: Int,
    val waiting: Int,
    val finished: Int,
    val history: Int,
) {
    operator fun get(filter: SessionFilter): Int =
        when (filter) {
            SessionFilter.ALL -> all
            SessionFilter.LIVE -> live
            SessionFilter.WAITING -> waiting
            SessionFilter.FINISHED -> finished
            SessionFilter.HISTORY -> history
        }
}

fun countSessions(sessions: List<SessionRecord>): SessionCounts =
    SessionCounts(
        all = sessions.size,
        live = sessions.count { it.state.presence == Presence.LIVE },
        waiting = sessions.count { it.state.activityStatus.isWaiting },
        finished =
            sessions.count {
                it.state.activityStatus in
                    setOf(
                        ActivityStatus.COMPLETED,
                        ActivityStatus.FAILED,
                        ActivityStatus.STOPPED,
                    )
            },
        history = sessions.count { it.state.presence == Presence.PERSISTED },
    )

fun filterSessions(
    sessions: List<SessionRecord>,
    filter: SessionFilter,
    deviceId: String? = null,
): List<SessionRecord> =
    sessions
        .asSequence()
        .filter { deviceId == null || it.deviceId == deviceId }
        .filter {
            when (filter) {
                SessionFilter.ALL -> true
                SessionFilter.LIVE -> it.state.presence == Presence.LIVE
                SessionFilter.WAITING -> it.state.activityStatus.isWaiting
                SessionFilter.FINISHED ->
                    it.state.activityStatus in
                        setOf(
                            ActivityStatus.COMPLETED,
                            ActivityStatus.FAILED,
                            ActivityStatus.STOPPED,
                        )
                SessionFilter.HISTORY -> it.state.presence == Presence.PERSISTED
            }
        }
        .sortedWith(
            compareByDescending<SessionRecord> { it.state.activityStatus.isWaiting }
                .thenByDescending { it.observedAt }
                .thenByDescending { it.sequence },
        )
        .toList()

data class AppConfig(
    val gatewayBase: String,
    val accessToken: String,
    val developmentUserId: String,
) {
    fun normalized(): AppConfig =
        copy(
            gatewayBase = gatewayBase.trim().trimEnd('/'),
            accessToken = accessToken.trim(),
            developmentUserId = developmentUserId.trim(),
        )

    fun validationError(): String? {
        val normalized = normalized()
        val uri =
            runCatching { URI(normalized.gatewayBase) }.getOrNull()
                ?: return "Gateway 地址格式无效"
        val host = uri.host?.lowercase() ?: return "Gateway 地址缺少主机名"
        val loopback = host == "127.0.0.1" || host == "localhost"
        if (uri.scheme != "https" && !(uri.scheme == "http" && loopback)) {
            return "远程 Gateway 必须使用 HTTPS"
        }
        if (uri.userInfo != null || uri.query != null || uri.fragment != null) {
            return "Gateway 地址不能包含凭据、查询参数或片段"
        }
        if (!loopback && normalized.accessToken.isBlank()) {
            return "生产 Gateway 必须填写访问令牌"
        }
        if (loopback && normalized.accessToken.isBlank() && normalized.developmentUserId.isBlank()) {
            return "请填写访问令牌；本机开发模式可填写开发用户"
        }
        if (normalized.accessToken.any { it.isISOControl() }) {
            return "访问令牌包含非法控制字符"
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
        val LocalDevelopment =
            AppConfig(
                gatewayBase = "http://127.0.0.1:3001",
                accessToken = "",
                developmentUserId = "local-user",
            )
    }
}
