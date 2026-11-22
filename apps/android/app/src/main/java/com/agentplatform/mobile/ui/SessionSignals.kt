package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.DiscoveryCandidate
import com.agentplatform.mobile.data.PendingApproval
import com.agentplatform.mobile.data.PendingQuestion
import com.agentplatform.mobile.data.RemoteEvent
import com.agentplatform.mobile.data.objects
import com.agentplatform.mobile.data.optNullableString
import com.agentplatform.mobile.data.paramsOf
import org.json.JSONObject

// =========================================================================
// Session side-signals
//
// Extracts turn failure, context window usage, pending approvals, and pending
// questions from the session event stream.
// =========================================================================

private fun applyFailureOnTurnCompleted(
    current: TurnFailure?,
    event: JSONObject,
    params: JSONObject?,
): TurnFailure? {
    val turnObj = params?.optJSONObject("turn")
    val completedTurnId =
        turnObj?.optString("id")?.takeIf { it.isNotEmpty() }
            ?: params?.optString("turnId")?.takeIf { it.isNotEmpty() }
    val turnStatus =
        turnObj?.optString("status")?.takeIf { it.isNotEmpty() }
            ?: params?.optString("status")?.takeIf { it.isNotEmpty() }
    val execState = event.optJSONObject("binding")?.optString("execution_state").orEmpty()
    val turnErrMsg =
        turnObj?.optJSONObject("error")?.optString("message")?.trim()?.takeIf { it.isNotEmpty() }
            ?: params?.optJSONObject("error")?.optString("message")?.trim()?.takeIf { it.isNotEmpty() }

    if (turnErrMsg != null) {
        return TurnFailure(turnId = completedTurnId ?: current?.turnId, message = turnErrMsg, retrying = false)
    }
    if (turnStatus == "failed" || turnStatus == "error" || execState == "error") {
        return current?.copy(retrying = false)
            ?: TurnFailure(
                turnId = completedTurnId,
                message = "执行异常中断，未返回具体原因。",
                retrying = false,
            )
    }
    // Preserve a final (non-retrying) `error` event emitted immediately before
    // `turn/completed` of the same turn so `TurnFailureCard` does not vanish
    // at turn end; it clears on the next `turn/started`.
    if (
        current != null &&
        !current.retrying &&
        (completedTurnId == null || current.turnId == null || completedTurnId == current.turnId)
    ) {
        return current
    }
    return null
}

private fun positiveLong(obj: JSONObject?, vararg keys: String): Long? {
    if (obj == null) return null
    for (key in keys) {
        val v = obj.optLong(key, 0L)
        if (v > 0L) return v
    }
    return null
}

private fun parseReportedTokenUsage(
    params: JSONObject?,
    candidate: DiscoveryCandidate?,
): ContextUsage? {
    if (params == null) return null
    val usage =
        params.optJSONObject("tokenUsage")
            ?: params.optJSONObject("token_usage")
            ?: params.optJSONObject("usage")
    val info = params.optJSONObject("info") ?: params.optJSONObject("turn")
    val last =
        usage?.optJSONObject("last")
            ?: usage?.optJSONObject("last_token_usage")
            ?: info?.optJSONObject("last_token_usage")
            ?: info?.optJSONObject("usage")
    val values = last ?: usage
    val used =
        positiveLong(
            values,
            "used",
            "context_used_tokens",
            "contextUsedTokens",
            "input_tokens",
            "inputTokens",
            "total_tokens",
            "totalTokens",
        ) ?: positiveLong(usage, "used") ?: return null
    val window =
        positiveLong(
            usage,
            "window",
            "modelContextWindow",
            "model_context_window",
            "contextWindowTokens",
            "context_window_tokens",
        ) ?: positiveLong(values, "window")
            ?: positiveLong(info, "model_context_window", "context_window_tokens")
            ?: positiveLong(params, "model_context_window", "context_window_tokens")
            ?: candidate?.inferredContextWindow?.takeIf { it > 0L }
    return ContextUsage(used = used, window = window)
}

private fun fallbackCandidateUsage(candidate: DiscoveryCandidate?): ContextUsage? {
    if (candidate == null) return null
    val fallbackWindow = candidate.inferredContextWindow
    val measured = candidate.contextUsedTokens
    val used =
        if (measured != null && measured > 0) {
            measured
        } else {
            val cumulative = candidate.promptTokens ?: candidate.totalTokens
            if (cumulative == null || cumulative <= 0) return null
            val calls = (candidate.requestCount ?: 1L).coerceAtLeast(1L)
            val perCall = cumulative / calls
            if (fallbackWindow > 0) minOf(perCall, fallbackWindow) else perCall
        }
    if (used <= 0) return null
    return ContextUsage(used = used, window = fallbackWindow.takeIf { it > 0 })
}

fun latestTurnFailure(events: List<RemoteEvent>, hubSessionId: String): TurnFailure? {
    var failure: TurnFailure? = null
    for (envelope in events) {
        val event = envelope.event ?: continue
        val bindingHub = event.optJSONObject("binding")?.optNullableString("hub_session_id")
        if (bindingHub != null && bindingHub != hubSessionId) continue
        when (event.optString("kind")) {
            "turn/started" -> failure = null
            "turn/completed" ->
                failure = applyFailureOnTurnCompleted(failure, event, envelope.paramsOf())
            "error" -> {
                val params = envelope.paramsOf() ?: continue
                val error = params.optJSONObject("error") ?: continue
                val message = error.optString("message").trim()
                if (message.isEmpty()) continue
                failure =
                    TurnFailure(
                        turnId = params.optString("turnId").takeIf { it.isNotEmpty() },
                        message = message,
                        retrying = params.optBoolean("willRetry", false),
                    )
            }
        }
    }
    return failure
}

fun latestContextUsage(
    events: List<RemoteEvent>,
    hubSessionId: String,
    candidate: DiscoveryCandidate? = null,
): ContextUsage? {
    var usage: ContextUsage? = null
    for (envelope in events) {
        val event = envelope.event ?: continue
        val bindingHub = event.optJSONObject("binding")?.optNullableString("hub_session_id")
        if (bindingHub != null && bindingHub != hubSessionId) continue
        if (event.optString("kind") != "thread/tokenUsage/updated") continue
        parseReportedTokenUsage(envelope.paramsOf(), candidate)?.let { parsed ->
            usage = parsed
        }
    }
    return usage ?: fallbackCandidateUsage(candidate)
}

fun pendingApprovals(events: List<RemoteEvent>, hubSessionId: String): List<PendingApproval> {
    val pending = LinkedHashMap<String, PendingApproval>()
    for (envelope in events) {
        val payload = envelope.payload
        when (payload.optString("type")) {
            "ready" -> {
                val reported = payload.optJSONArray("pending_approvals")
                if (reported != null) {
                    pending.clear()
                    reported
                        .objects()
                        .map(PendingApproval::parse)
                        .filter { it.hubSessionId == hubSessionId }
                        .forEach { pending[it.approvalId] = it }
                }
            }
            "host_lost" -> pending.clear()
            "receipt" -> {
                val receipt = payload.optJSONObject("receipt") ?: continue
                if (receipt.optString("status") == "accepted") {
                    receipt.optJSONObject("result")
                        ?.optString("approval_id")
                        ?.takeIf { it.isNotEmpty() }
                        ?.let(pending::remove)
                }
            }
            "event" -> {
                val event = payload.optJSONObject("event") ?: continue
                when (event.optString("kind")) {
                    "approval_requested" ->
                        event.optJSONObject("approval")
                            ?.let(PendingApproval::parse)
                            ?.takeIf { it.hubSessionId == hubSessionId }
                            ?.let { pending[it.approvalId] = it }
                    "approval_resolved" ->
                        event.optString("approval_id")
                            .takeIf { it.isNotEmpty() }
                            ?.let(pending::remove)
                    "turn/completed" ->
                        if (
                            event.optJSONObject("binding")
                                ?.optString("hub_session_id") == hubSessionId
                        ) {
                            pending.clear()
                        }
                }
            }
        }
    }
    return pending.values.toList()
}

fun pendingQuestions(events: List<RemoteEvent>, hubSessionId: String): List<PendingQuestion> {
    val pending = LinkedHashMap<String, PendingQuestion>()
    for (envelope in events) {
        val payload = envelope.payload
        when (payload.optString("type")) {
            "ready" -> {
                val reported = payload.optJSONArray("pending_questions")
                if (reported != null) {
                    pending.clear()
                    reported
                        .objects()
                        .map(PendingQuestion::parse)
                        .filter { it.hubSessionId == hubSessionId }
                        .forEach { pending[it.questionId] = it }
                }
            }
            "host_lost" -> pending.clear()
            "receipt" -> {
                val receipt = payload.optJSONObject("receipt") ?: continue
                if (receipt.optString("status") == "accepted") {
                    receipt.optJSONObject("result")
                        ?.optString("question_id")
                        ?.takeIf { it.isNotEmpty() }
                        ?.let(pending::remove)
                }
            }
            "event" -> {
                val event = payload.optJSONObject("event") ?: continue
                when (event.optString("kind")) {
                    "question_requested" ->
                        event.optJSONObject("question")
                            ?.let(PendingQuestion::parse)
                            ?.takeIf { it.hubSessionId == hubSessionId }
                            ?.let { pending[it.questionId] = it }
                    "question_resolved" ->
                        event.optString("question_id")
                            .takeIf { it.isNotEmpty() }
                            ?.let(pending::remove)
                    "turn/completed" ->
                        if (
                            event.optJSONObject("binding")
                                ?.optString("hub_session_id") == hubSessionId
                        ) {
                            pending.clear()
                        }
                }
            }
        }
    }
    return pending.values.toList()
}

data class SessionSignals(
    val approvals: List<PendingApproval>,
    val questions: List<PendingQuestion>,
    val turnFailure: TurnFailure?,
    val usage: ContextUsage?,
)

fun analyzeSessionSignals(
    events: List<RemoteEvent>,
    hubSessionId: String,
    candidate: DiscoveryCandidate? = null,
): SessionSignals {
    val approvals = LinkedHashMap<String, PendingApproval>()
    val questions = LinkedHashMap<String, PendingQuestion>()
    var failure: TurnFailure? = null
    var usage: ContextUsage? = null

    for (envelope in events) {
        val payload = envelope.payload
        when (payload.optString("type")) {
            "ready" -> {
                val reportedApprovals = payload.optJSONArray("pending_approvals")
                if (reportedApprovals != null) {
                    approvals.clear()
                    reportedApprovals
                        .objects()
                        .map(PendingApproval::parse)
                        .filter { it.hubSessionId == hubSessionId }
                        .forEach { approvals[it.approvalId] = it }
                }
                val reportedQuestions = payload.optJSONArray("pending_questions")
                if (reportedQuestions != null) {
                    questions.clear()
                    reportedQuestions
                        .objects()
                        .map(PendingQuestion::parse)
                        .filter { it.hubSessionId == hubSessionId }
                        .forEach { questions[it.questionId] = it }
                }
            }
            "host_lost" -> {
                approvals.clear()
                questions.clear()
            }
            "receipt" -> {
                val receipt = payload.optJSONObject("receipt") ?: continue
                if (receipt.optString("status") == "accepted") {
                    val res = receipt.optJSONObject("result")
                    res?.optString("approval_id")?.takeIf { it.isNotEmpty() }?.let(approvals::remove)
                    res?.optString("question_id")?.takeIf { it.isNotEmpty() }?.let(questions::remove)
                }
            }
            "event" -> {
                val event = payload.optJSONObject("event") ?: continue
                val bindingObj = event.optJSONObject("binding")
                val bindingHubExact = bindingObj?.optString("hub_session_id")
                val bindingHubNullable = bindingObj?.optNullableString("hub_session_id")
                val matchesHubOrGlobal = bindingHubNullable == null || bindingHubNullable == hubSessionId

                when (event.optString("kind")) {
                    "approval_requested" ->
                        event.optJSONObject("approval")
                            ?.let(PendingApproval::parse)
                            ?.takeIf { it.hubSessionId == hubSessionId }
                            ?.let { approvals[it.approvalId] = it }
                    "approval_resolved" ->
                        event.optString("approval_id")
                            .takeIf { it.isNotEmpty() }
                            ?.let(approvals::remove)
                    "question_requested" ->
                        event.optJSONObject("question")
                            ?.let(PendingQuestion::parse)
                            ?.takeIf { it.hubSessionId == hubSessionId }
                            ?.let { questions[it.questionId] = it }
                    "question_resolved" ->
                        event.optString("question_id")
                            .takeIf { it.isNotEmpty() }
                            ?.let(questions::remove)
                    "turn/started" ->
                        if (matchesHubOrGlobal) failure = null
                    "turn/completed" -> {
                        if (bindingHubExact == hubSessionId) {
                            approvals.clear()
                            questions.clear()
                        }
                        if (matchesHubOrGlobal) {
                            failure = applyFailureOnTurnCompleted(failure, event, envelope.paramsOf())
                        }
                    }
                    "error" ->
                        if (matchesHubOrGlobal) {
                            val params = envelope.paramsOf() ?: continue
                            val err = params.optJSONObject("error") ?: continue
                            val message = err.optString("message").trim()
                            if (message.isNotEmpty()) {
                                failure =
                                    TurnFailure(
                                        turnId = params.optString("turnId").takeIf { it.isNotEmpty() },
                                        message = message,
                                        retrying = params.optBoolean("willRetry", false),
                                    )
                            }
                        }
                    "thread/tokenUsage/updated" ->
                        if (matchesHubOrGlobal) {
                            parseReportedTokenUsage(envelope.paramsOf(), candidate)?.let { parsed ->
                                usage = parsed
                            }
                        }
                }
            }
        }
    }

    return SessionSignals(
        approvals = approvals.values.toList(),
        questions = questions.values.toList(),
        turnFailure = failure,
        usage = usage ?: fallbackCandidateUsage(candidate),
    )
}
