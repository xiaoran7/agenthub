package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.AgentPlugin
import com.agentplatform.mobile.data.DiscoveryCandidate
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.PendingApproval
import com.agentplatform.mobile.data.PendingQuestion
import com.agentplatform.mobile.data.Snapshot
import com.agentplatform.mobile.data.displayEngineLabel
import com.agentplatform.mobile.data.engineOfSession

// =========================================================================
// Session tree
//
// The same four-level hierarchy the browser console shows:
//   device -> agent -> workspace -> session
//
// Pure Kotlin so ordering, title sanitization, and workspace decoding are
// unit-testable without an emulator.
// =========================================================================

/** Folder order aligned with Web `session-tree.ts`. */
private val providerOrder =
    mapOf(
        EngineKind.CODEX to 0,
        EngineKind.CLAUDE_CODE to 1,
        EngineKind.CLAUDE_DESKTOP to 2,
        EngineKind.ANTIGRAVITY to 3,
        EngineKind.DEEPSEEK_HARNESS to 4,
        EngineKind.PLUGIN to 5,
    )

private val WS_TOKEN_REGEX = Regex("^ws-(.+)-([0-9a-f]{16})$")
private val ALNUM_OR_CJK_REGEX = Regex("[\\p{L}\\p{N}]")
private val COMMAND_NAME_TAG_REGEX =
    Regex("<command-name>([\\s\\S]*?)</command-name>", RegexOption.IGNORE_CASE)
private val COMMAND_META_TAGS_REGEX =
    Regex(
        "<(?:command-message|command-name|command-args|local-command-stdout|local-command-caveat|user_settings_change|environment_context|turn_aborted|context_summary)>[\\s\\S]*?</(?:command-message|command-name|command-args|local-command-stdout|local-command-caveat|user_settings_change|environment_context|turn_aborted|context_summary)>",
        RegexOption.IGNORE_CASE,
    )
private val XML_TAG_REGEX = Regex("</?[a-zA-Z][a-zA-Z0-9_-]*(?:\\s[^>]*)?>")
private val MD_LINK_REGEX = Regex("\\[([^\\]]+)]\\([^)]*\\)")
private val LEADING_HEADING_REGEX = Regex("^#+\\s*")
private val MULTI_SPACE_REGEX = Regex("\\s+")

/**
 * Decodes a daemon `ws-...-<16hex>` workspace token or filesystem path into a
 * clean folder basename. Handles `___` encoded path separators and redacted CJK
 * underscore-only slugs (`ws-__-<hash>` -> `"本地目录 · xxxx"`).
 */
fun workspaceLabel(workspaceId: String): String {
    val raw = workspaceId.trim()
    if (raw.isEmpty()) return workspaceId
    if (raw.startsWith("ws-")) {
        val match = WS_TOKEN_REGEX.find(raw)
        val slugPart = match?.groupValues?.get(1) ?: raw.removePrefix("ws-")
        val hashPart = match?.groupValues?.get(2).orEmpty()
        val normalizedSlug =
            if (slugPart.contains("___")) {
                slugPart
                    .replace(Regex("_{3,}"), "/")
                    .trimEnd('/', '\\')
                    .substringAfterLast('/')
                    .substringAfterLast('\\')
            } else {
                slugPart
            }.trim()
        if (normalizedSlug.isEmpty() || !ALNUM_OR_CJK_REGEX.containsMatchIn(normalizedSlug)) {
            return if (hashPart.isNotEmpty()) {
                "本地目录 · ${hashPart.take(4)}"
            } else {
                "本地目录"
            }
        }
        return normalizedSlug
    }
    val cleaned = raw.removePrefix("\\\\?\\").trimEnd('/', '\\')
    if (cleaned.isEmpty()) return raw
    return cleaned.substringAfterLast('/').substringAfterLast('\\').ifBlank { cleaned }
}

/**
 * Extracts a human-readable relative/collapsed path for a workspace id,
 * preferring the Runner's `workspace_labels` map and never exposing raw
 * `ws-<slug>-<16hex>` hashes.
 */
fun workspaceDisplayPath(
    workspaceId: String,
    workspaceLabels: Map<String, String> = emptyMap(),
): String? {
    val raw = workspaceId.trim()
    if (raw.isEmpty()) return null
    workspaceLabels[raw]?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
    if (raw.startsWith("ws-")) {
        val match = WS_TOKEN_REGEX.find(raw)
        val slugPart = match?.groupValues?.get(1) ?: return null
        if (slugPart.contains("___")) {
            val parts =
                slugPart
                    .replace(Regex("_{3,}"), "/")
                    .split('/')
                    .map { it.trim('_').trim() }
                    .filter { it.isNotEmpty() }
            if (parts.size >= 2) {
                return if (parts[0].length == 1 && parts[0][0].isLetter()) {
                    "${parts[0].uppercase()}: ${parts.drop(1).joinToString("/")}"
                } else {
                    parts.joinToString("/")
                }
            }
        }
        return null
    }
    val cleaned = raw.removePrefix("\\\\?\\").replace('\\', '/').trimEnd('/')
    return cleaned.takeIf { it.isNotEmpty() }
}

fun workspaceShortHash(workspaceId: String): String? =
    WS_TOKEN_REGEX.find(workspaceId.trim())?.groupValues?.get(2)?.take(4)

/**
 * Formats the secondary line under a workspace name in pickers: always shows
 * the target machine plus a human-readable path (or session count / authorized
 * hint on older Runners), never raw `ws-...-<16hex>` gibberish.
 */
fun workspaceChoiceSubtitle(
    workspaceId: String,
    workspaceLabels: Map<String, String>,
    deviceName: String,
    sessionCount: Int = 0,
    duplicated: Boolean = false,
): String {
    val path = workspaceDisplayPath(workspaceId, workspaceLabels)
    val hashTag = if (duplicated) workspaceShortHash(workspaceId)?.let { "#$it" } else null
    val parts = buildList {
        if (deviceName.isNotBlank()) add(deviceName)
        if (!path.isNullOrBlank()) {
            add(path)
        } else if (sessionCount > 0) {
            add("$sessionCount 个历史会话")
        } else {
            add("已授权目录")
        }
        if (hashTag != null) add(hashTag)
    }
    return parts.joinToString(" · ")
}

/**
 * Strips Claude/CLI internal XML tags (`<command-name>`, `<command-message>`,
 * `<command-args>`), stray HTML/XML wrappers, Markdown links/backticks, and
 * heading markers so session rows never expose raw XML markup.
 */
fun sanitizeSessionTitle(raw: String?): String {
    if (raw.isNullOrBlank()) return ""
    val commandName =
        COMMAND_NAME_TAG_REGEX.find(raw)?.groupValues?.get(1)?.trim().orEmpty()
    val stripped =
        raw
            .replace(COMMAND_META_TAGS_REGEX, " ")
            .replace(XML_TAG_REGEX, " ")
            .replace(MD_LINK_REGEX, "$1")
            .replace("`", "")
            .trim()
            .replace(LEADING_HEADING_REGEX, "")
            .replace(MULTI_SPACE_REGEX, " ")
            .trim()
    if (stripped.isNotEmpty()) return stripped
    if (commandName.isNotEmpty()) return commandName
    return ""
}

fun deviceLabel(runnerId: String, deviceName: String?): String {
    val named = deviceName?.trim().orEmpty()
    if (named.isNotEmpty()) return named
    return "电脑 · ${runnerId.takeLast(6)}"
}

sealed interface TreeEntry {
    val key: String
    val runnerId: String
    val providerSessionId: String
    val workspaceId: String
    val provider: EngineKind
    val title: String
    val lastActivityAtMs: Long?
    val deviceNameLabel: String
    val agentLabel: String

    /** A session this phone controls right now. */
    data class Active(
        override val key: String,
        override val runnerId: String,
        override val providerSessionId: String,
        override val workspaceId: String,
        override val provider: EngineKind,
        override val title: String,
        override val lastActivityAtMs: Long?,
        val hubSessionId: String,
        val online: Boolean,
        val controlConnected: Boolean,
        val executionState: ExecutionState,
        override val deviceNameLabel: String = "",
        override val agentLabel: String = provider.label,
    ) : TreeEntry

    /** A conversation found on disk; resume it to make it controllable. */
    data class Candidate(
        override val key: String,
        override val runnerId: String,
        override val providerSessionId: String,
        override val workspaceId: String,
        override val provider: EngineKind,
        override val title: String,
        override val lastActivityAtMs: Long?,
        val resumable: Boolean,
        val model: String? = null,
        val totalTokens: Long? = null,
        override val deviceNameLabel: String = "",
        override val agentLabel: String = provider.label,
    ) : TreeEntry
}

fun entryActivity(entry: TreeEntry): Long {
    val stamp = entry.lastActivityAtMs?.takeIf { it > 0 } ?: 0L
    return when (entry) {
        is TreeEntry.Active ->
            when {
                entry.controlConnected -> CONTROL_CONNECTED_TIER + stamp
                entry.online -> HOST_ONLINE_TIER + stamp
                else -> stamp
            }
        is TreeEntry.Candidate -> stamp
    }
}

private const val CONTROL_CONNECTED_TIER = 2_000_000_000_000_000L
private const val HOST_ONLINE_TIER = 1_000_000_000_000_000L

data class WorkspaceGroup(
    val id: String,
    val label: String,
    val entries: List<TreeEntry>,
)

data class AgentGroup(
    val provider: EngineKind,
    val label: String,
    val workspaces: List<WorkspaceGroup>,
    val count: Int,
)

data class DeviceGroup(
    val runnerId: String,
    val label: String,
    val online: Boolean,
    val agents: List<AgentGroup>,
    val count: Int,
)

/** Cross-device waiting item enriched with inline approval/question metadata. */
data class AttentionItem(
    val entry: TreeEntry.Active,
    val approval: PendingApproval?,
    val question: PendingQuestion?,
)

internal fun mergeHostPlugins(
    hostPlugins: List<AgentPlugin>?,
    controllerPlugins: List<AgentPlugin>,
): List<AgentPlugin> {
    if (hostPlugins.isNullOrEmpty()) return controllerPlugins
    if (controllerPlugins.isEmpty()) return hostPlugins
    val byId = LinkedHashMap<String, AgentPlugin>(hostPlugins.size + controllerPlugins.size)
    for (plugin in hostPlugins) byId[plugin.id] = plugin
    for (plugin in controllerPlugins) byId.putIfAbsent(plugin.id, plugin)
    return byId.values.toList()
}

fun buildSessionTree(
    snapshot: Snapshot,
    providerFilter: EngineKind? = null,
    plugins: List<AgentPlugin> = emptyList(),
): List<DeviceGroup> {
    val entries = mutableListOf<TreeEntry>()
    val seenActiveKeys = HashSet<String>()
    val activeProviderKeys = HashSet<String>()
    val revoked = snapshot.hosts.filter { it.revoked }.mapTo(HashSet()) { it.runnerId }
    val hostMap = snapshot.hosts.associateBy { it.runnerId }
    val discovery = discoveryIndex(snapshot)

    for (session in snapshot.sessions) {
        if (session.runnerId in revoked) continue
        val binding = session.binding
        val activeKey = "active:${session.runnerId}/${binding.hubSessionId}"
        if (!seenActiveKeys.add(activeKey)) continue
        val providerKey = "${session.runnerId}/${binding.providerSessionId}"
        if (binding.providerSessionId.isNotBlank()) activeProviderKeys += providerKey
        val host = hostMap[session.runnerId]
        val effectivePlugins = mergeHostPlugins(host?.hello?.plugins, plugins)
        val discovered = discovery[providerKey]
        val reportedProvider = session.provider ?: discovered?.provider
        val provider = engineOfSession(binding.providerSessionId, reportedProvider, effectivePlugins)
        if (providerFilter != null && providerFilter != provider) continue
        val cleanTitle =
            sanitizeSessionTitle(binding.title)
                .ifBlank { sanitizeSessionTitle(discovered?.title) }
                .ifBlank { FALLBACK_TITLE }
        entries +=
            TreeEntry.Active(
                key = activeKey,
                runnerId = session.runnerId,
                providerSessionId = binding.providerSessionId,
                workspaceId = binding.workspacePath,
                provider = provider,
                title = cleanTitle,
                lastActivityAtMs = discovered?.lastActivityAtMs,
                hubSessionId = binding.hubSessionId,
                online = session.online,
                controlConnected = binding.controlConnected,
                executionState = binding.executionState,
                deviceNameLabel = deviceLabel(session.runnerId, host?.hello?.deviceName),
                agentLabel = displayEngineLabel(provider, binding.providerSessionId, reportedProvider, effectivePlugins),
            )
    }

    val seenCandidateKeys = HashSet<String>()
    for (host in snapshot.hosts) {
        if (host.revoked) continue
        val devName = deviceLabel(host.runnerId, host.hello?.deviceName)
        val effectivePlugins = mergeHostPlugins(host.hello?.plugins, plugins)
        for (candidate in host.hello?.candidates.orEmpty()) {
            val providerKey = "${host.runnerId}/${candidate.providerSessionId}"
            if (providerKey in activeProviderKeys) continue
            if (!seenCandidateKeys.add(providerKey)) continue
            val provider = engineOfSession(candidate.providerSessionId, candidate.provider, effectivePlugins)
            if (providerFilter != null && providerFilter != provider) continue
            val cleanTitle = sanitizeSessionTitle(candidate.title).ifBlank { FALLBACK_TITLE }
            entries +=
                TreeEntry.Candidate(
                    key = "candidate:$providerKey",
                    runnerId = host.runnerId,
                    providerSessionId = candidate.providerSessionId,
                    workspaceId = candidate.workspacePath,
                    provider = provider,
                    title = cleanTitle,
                    lastActivityAtMs = candidate.lastActivityAtMs,
                    resumable = candidate.resumable,
                    model = candidate.model,
                    totalTokens = candidate.totalTokens,
                    deviceNameLabel = devName,
                    agentLabel =
                        displayEngineLabel(provider, candidate.providerSessionId, candidate.provider, effectivePlugins),
                )
        }
    }

    val devices = LinkedHashMap<String, HostDevice>()
    for (host in snapshot.hosts) {
        if (host.revoked) continue
        devices[host.runnerId] = host
    }
    val grouped =
        LinkedHashMap<String, MutableMap<EngineKind, MutableMap<String, MutableList<TreeEntry>>>>()
    for (entry in entries) {
        grouped
            .getOrPut(entry.runnerId) { LinkedHashMap() }
            .getOrPut(entry.provider) { LinkedHashMap() }
            .getOrPut(entry.workspaceId) { mutableListOf() }
            .add(entry)
    }
    for (runnerId in grouped.keys) {
        if (runnerId in revoked) continue
        devices.putIfAbsent(runnerId, HostDevice(runnerId, false, false, null))
    }

    return devices.values
        .map { host ->
            val byProvider = grouped[host.runnerId].orEmpty()
            val agents =
                byProvider.entries
                    .sortedWith(
                        compareBy<Map.Entry<EngineKind, MutableMap<String, MutableList<TreeEntry>>>> {
                            providerOrder[it.key] ?: Int.MAX_VALUE
                        }.thenBy { it.key.label },
                    )
                    .map { (provider, workspaces) ->
                        val groups =
                            workspaces.entries
                                .map { (id, items) ->
                                    WorkspaceGroup(
                                        id = id,
                                        label = workspaceLabel(id),
                                        entries = items.sortedWith(ENTRY_ORDER),
                                    )
                                }
                                .sortedWith(
                                    compareByDescending<WorkspaceGroup> { groupActivity(it) }
                                        .thenBy { it.label },
                                )
                        AgentGroup(
                            provider = provider,
                            label = provider.label,
                            workspaces = groups,
                            count = groups.sumOf { it.entries.size },
                        )
                    }
            DeviceGroup(
                runnerId = host.runnerId,
                label = deviceLabel(host.runnerId, host.hello?.deviceName),
                online = host.online,
                agents = agents,
                count = agents.sumOf { it.count },
            )
        }
        .sortedWith(
            compareByDescending<DeviceGroup> { it.online }.thenBy { it.label },
        )
}

internal fun groupActivity(group: WorkspaceGroup): Long =
    group.entries.maxOfOrNull(::entryActivity) ?: 0L

private val ENTRY_ORDER: Comparator<TreeEntry> =
    compareByDescending<TreeEntry> { entryActivity(it) }.thenBy { it.title }

private const val FALLBACK_TITLE = "新会话"

private fun discoveryIndex(snapshot: Snapshot): Map<String, DiscoveryCandidate> {
    val index = HashMap<String, DiscoveryCandidate>()
    for (host in snapshot.hosts) {
        for (candidate in host.hello?.candidates.orEmpty()) {
            index["${host.runnerId}/${candidate.providerSessionId}"] = candidate
        }
    }
    return index
}

fun entryKeyOf(entry: TreeEntry): String =
    when (entry) {
        is TreeEntry.Active -> "${entry.runnerId}/${entry.hubSessionId}"
        is TreeEntry.Candidate -> entry.key
    }

fun flattenTree(devices: List<DeviceGroup>): List<TreeEntry> =
    devices.flatMap { device -> device.agents.flatMap { it.workspaces.flatMap { it.entries } } }

fun attentionEntries(tree: List<DeviceGroup>): List<TreeEntry> =
    flattenTree(tree)
        .filterIsInstance<TreeEntry.Active>()
        .filter { it.controlConnected && it.executionState.waiting }
        .sortedByDescending { entryActivity(it) }

/**
 * Builds the global cross-device attention queue (never hidden by `providerFilter`),
 * pairing waiting sessions with any inline `PendingApproval` / `PendingQuestion`
 * published in `host.hello`.
 */
fun buildAttentionQueue(
    snapshot: Snapshot,
    plugins: List<AgentPlugin> = emptyList(),
): List<AttentionItem> {
    val fullTree = buildSessionTree(snapshot, providerFilter = null, plugins = plugins)
    val approvalsByKey = HashMap<String, PendingApproval>()
    val questionsByKey = HashMap<String, PendingQuestion>()
    for (host in snapshot.hosts) {
        if (host.revoked) continue
        host.hello?.pendingApprovalsList?.forEach { appr ->
            approvalsByKey.putIfAbsent("${host.runnerId}/${appr.hubSessionId}", appr)
        }
        host.hello?.pendingQuestionsList?.forEach { q ->
            questionsByKey.putIfAbsent("${host.runnerId}/${q.hubSessionId}", q)
        }
    }
    val activeByHubKey =
        flattenTree(fullTree)
            .filterIsInstance<TreeEntry.Active>()
            .associateBy { "${it.runnerId}/${it.hubSessionId}" }

    val selected = LinkedHashMap<String, AttentionItem>()
    for ((hubKey, active) in activeByHubKey) {
        val appr = approvalsByKey[hubKey]
        val q = questionsByKey[hubKey]
        if (active.online && active.controlConnected && (active.executionState.waiting || appr != null || q != null)) {
            selected[hubKey] = AttentionItem(entry = active, approval = appr, question = q)
        }
    }
    return selected.values.sortedByDescending { entryActivity(it.entry) }
}
