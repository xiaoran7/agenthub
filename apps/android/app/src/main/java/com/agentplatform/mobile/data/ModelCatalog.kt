package com.agentplatform.mobile.data

import java.util.Locale

// =========================================================================
// Engine model catalog
//
// `ready.models` is the Host's answer to "what can this engine run". The
// contract deliberately leaves it as `unknown` (`RemoteReady.models`), so this
// file is where the shape is narrowed once, in pure Kotlin, for the whole app.
//
// Two facts drive the design and both come from the Host, not from a guess:
//
//   * `models.agents` is a per-engine map. An id taken from one engine's list
//     is refused by another, so the phone only ever offers the list of the
//     engine that will actually receive the command.
//   * A Host that predates `agents` still sends the flat `data` list, and that
//     list belongs to whichever provider answered — for Codex it *is* the
//     `model/list` RPC answer, so it is offered for Codex and nowhere else.
//
// Pure Kotlin (no `org.json`) so the selection rule is unit-testable without
// an emulator.
// =========================================================================

/** One entry the Host advertised. `id` is what goes on the wire. */
data class CatalogModel(
    val id: String,
    val displayName: String,
) {
    /** What the picker shows; a blank display name never reaches the UI. */
    val label: String
        get() = displayName.ifBlank { id }
}

/** `ready.models`, narrowed to what the phone reads. */
data class ModelCatalog(
    /** Flat list, the pre-`agents` shape. Only meaningful for Codex. */
    val flat: List<CatalogModel>,
    /** Engine key -> that engine's own models. */
    val agents: Map<String, List<CatalogModel>>,
) {
    val isEmpty: Boolean
        get() = flat.isEmpty() && agents.values.all { it.isEmpty() }

    companion object {
        val Empty = ModelCatalog(emptyList(), emptyMap())

        /**
         * Reads `hello.models`.
         *
         * Every field is optional because the contract types the whole object
         * as `unknown`: a Host that sends a different shape yields an empty
         * catalog rather than an exception, and the UI degrades to "no choice".
         *
         * Each entry is accepted under any of the four spellings the Host has
         * used across releases — `displayName`, `display_name`, `name`, `id` —
         * so a rename upstream does not blank every label at once.
         */
        fun parse(json: org.json.JSONObject?): ModelCatalog {
            if (json == null) return Empty
            fun entry(raw: org.json.JSONObject): CatalogModel? {
                val id =
                    raw.optNullableString("id")?.trim()
                        ?: raw.optNullableString("model")?.trim()
                        ?: return null
                val display =
                    raw.optNullableString("displayName")?.trim()
                        ?: raw.optNullableString("display_name")?.trim()
                        ?: raw.optNullableString("name")?.trim()
                        ?: id
                return CatalogModel(id = id, displayName = display)
            }
            fun list(raw: org.json.JSONArray?): List<CatalogModel> =
                raw?.objects()?.mapNotNull(::entry).orEmpty()
            val flat = list(json.optJSONArray("data"))
            val agents = LinkedHashMap<String, List<CatalogModel>>()
            val groups = json.optJSONObject("agents")
            groups?.keys()?.forEach { key ->
                list(groups.optJSONArray(key)).takeIf { it.isNotEmpty() }?.let { agents[key] = it }
            }
            return ModelCatalog(flat = flat, agents = agents)
        }
    }
}

/**
 * The engine a command will be sent to, and the folder its sessions are filed under.
 *
 * One type carries both jobs on purpose. They used to be two — [EngineKind] here
 * for the catalog and `ProviderKind` in the UI for the tree — and two copies of
 * the same classification drift: `cursor` and `cline` were hardcoded to the
 * Coding Assistant on one side and fell through to an id prefix on the other,
 * so a session could be filed under one engine while the model picker looked in
 * another's catalog and offered ids the Runner rejects with `invalid_field`.
 *
 * The phone does not invent these: they are the `connect.agent_type` values the
 * Runner accepts, plus the engines a resumed session can already belong to.
 * The row glyph is a separate mapping in the UI layer — see `ProviderGlyph.of` —
 * because a `data` type must not depend on a `ui` one.
 */
private val VALID_AGENT_SLUG = Regex("^[a-z][a-z0-9_-]{1,63}$")

data class LaunchAgentOption(
    val wire: String,
    val label: String,
    val engine: EngineKind,
    val isPlugin: Boolean = false,
)

enum class EngineKind(
    /** `connect.agent_type`, and the `provider` string a Host reports. */
    val wire: String,
    /** What the console shows. The tree folder is titled with it. */
    val label: String,
) {
    CODEX("codex", "Codex"),
    CLAUDE_CODE("claude_code", "Coding Assistant"),
    CLAUDE_DESKTOP("claude_desktop", "Claude Desktop"),
    ANTIGRAVITY("antigravity", "Antigravity"),
    DEEPSEEK_HARNESS("deepseek_harness", "DeepSeek Harness"),
    PLUGIN("plugin", "插件智能体"),
    ;

    /**
     * Keys read from `models.agents` for this native engine.
     *
     * Plugin engines dynamically read `models.agents[plugin_id]`.
     */
    val catalogKeys: List<String>
        get() =
            when (this) {
                CODEX -> listOf("codex")
                CLAUDE_CODE -> listOf("claude_code", "claude")
                CLAUDE_DESKTOP -> listOf("claude_desktop")
                ANTIGRAVITY -> listOf("antigravity")
                DEEPSEEK_HARNESS -> listOf("deepseek_harness")
                PLUGIN -> emptyList()
            }

    companion object {
        fun fromWire(value: String): EngineKind? = entries.firstOrNull { it.wire == value }

        /**
         * Engines offered when starting a brand-new remote session.
         *
         * `CLAUDE_DESKTOP` and `DEEPSEEK_HARNESS` are discovery/contract-only on
         * the Runner; external ACP plugins surface under [PLUGIN] when enabled.
         */
        fun launchable(enabledPluginIds: Set<String>? = null): List<EngineKind> {
            val base = listOf(CODEX, CLAUDE_CODE, ANTIGRAVITY)
            return if (!enabledPluginIds.isNullOrEmpty()) {
                base + PLUGIN
            } else {
                base
            }
        }

        /**
         * Full launch options combining the 3 launchable native engines with
         * installed, enabled, and launch-capable ACP plugins reported by the Runner.
         */
        fun launchableAgents(plugins: List<AgentPlugin> = emptyList()): List<LaunchAgentOption> {
            val nativeOptions =
                listOf(
                    LaunchAgentOption(CODEX.wire, CODEX.label, CODEX),
                    LaunchAgentOption(CLAUDE_CODE.wire, CLAUDE_CODE.label, CLAUDE_CODE),
                    LaunchAgentOption(ANTIGRAVITY.wire, ANTIGRAVITY.label, ANTIGRAVITY),
                )
            val pluginOptions =
                plugins
                    .filter { plugin ->
                        plugin.enabled &&
                            plugin.installed &&
                            (plugin.available || plugin.runtimeStatus == "ready" || plugin.runtimeStatus == "active") &&
                            (plugin.capabilities.launch || !plugin.capabilities.discover)
                    }
                    .sortedBy { it.id }
                    .map { plugin ->
                        LaunchAgentOption(
                            wire = plugin.id,
                            label = plugin.label,
                            engine = PLUGIN,
                            isPlugin = true,
                        )
                    }
            return nativeOptions + pluginOptions
        }
    }
}

/** Human-readable runtime label preserving plugin and ACP distinctions. */
fun displayEngineLabel(
    engine: EngineKind,
    providerSessionId: String = "",
    reported: String? = null,
    plugins: List<AgentPlugin> = emptyList(),
): String {
    val norm = reported?.trim()?.lowercase(Locale.US).orEmpty()
    val matchedPlugin =
        plugins.firstOrNull { p ->
            p.id.lowercase(Locale.US) == norm ||
                p.aliases.any { it.lowercase(Locale.US) == norm } ||
                (norm.isEmpty() && providerSessionId.isNotEmpty() && p.allSessionPrefixes.any { providerSessionId.startsWith(it) })
        }
    if (matchedPlugin != null) return matchedPlugin.label
    return when {
        norm.startsWith("acp") || providerSessionId.startsWith("acp_") -> "ACP"
        norm == "gemini" || norm == "gemini_cli" || providerSessionId.startsWith("gemini_") ->
            "Gemini CLI"
        norm == "zcode" || providerSessionId.startsWith("sess_") -> "ZCode"
        norm == "grok" || norm == "grok_build" || providerSessionId.startsWith("grok_") -> "Grok"
        engine == EngineKind.PLUGIN && norm.isNotEmpty() -> reported!!.trim()
        else -> engine.label
    }
}

/**
 * The engine of an existing session.
 *
 * The 5 native providers resolve to their dedicated [EngineKind]; any valid
 * non-native ACP plugin slug or plugin session prefix resolves to [EngineKind.PLUGIN].
 */
fun engineOfSession(
    providerSessionId: String,
    reported: String?,
    plugins: List<AgentPlugin> = emptyList(),
): EngineKind {
    val norm = reported?.trim()?.lowercase(Locale.US).orEmpty()
    return when (norm) {
        "codex" -> EngineKind.CODEX
        "claude_code", "claude" -> EngineKind.CLAUDE_CODE
        "claude_desktop" -> EngineKind.CLAUDE_DESKTOP
        "antigravity" -> EngineKind.ANTIGRAVITY
        "deepseek_harness", "deepseek", "deepseek-harness" -> EngineKind.DEEPSEEK_HARNESS
        "" ->
            when {
                providerSessionId.startsWith("claude_") -> EngineKind.CLAUDE_CODE
                providerSessionId.startsWith("antigravity_") -> EngineKind.ANTIGRAVITY
                providerSessionId.startsWith("deepseek_") -> EngineKind.DEEPSEEK_HARNESS
                providerSessionId.startsWith("sess_") ||
                    providerSessionId.startsWith("gemini_") ||
                    providerSessionId.startsWith("grok_") ||
                    plugins.any { p ->
                        p.allSessionPrefixes.any { providerSessionId.startsWith(it) }
                    } -> EngineKind.PLUGIN
                else -> EngineKind.CODEX
            }
        else ->
            if (VALID_AGENT_SLUG.matches(norm)) {
                EngineKind.PLUGIN
            } else {
                EngineKind.CODEX
            }
    }
}

/** Placeholder id the Host uses for "this engine has no selectable models". */
const val SYNTHETIC_MODEL: String = "<synthetic>"

/**
 * The models this engine accepts, in the order they should be offered.
 *
 * Returns an empty list when the Host said nothing for this engine — a phone
 * that guessed at model ids would send a string the Runner rejects with
 * `invalid_field`, so "no choice offered" is the correct outcome, not a
 * fallback to another engine's list.
 */
fun modelsForEngine(
    catalog: ModelCatalog,
    engine: EngineKind,
    reportedProvider: String? = null,
    providerSessionId: String = "",
): List<CatalogModel> {
    val seen = LinkedHashSet<String>()
    val models = mutableListOf<CatalogModel>()
    fun push(model: CatalogModel) {
        val id = model.id.trim()
        if (id.isEmpty() || id == SYNTHETIC_MODEL) return
        if (!seen.add(id.lowercase(Locale.US))) return
        models += CatalogModel(id = id, displayName = model.displayName.trim())
    }
    val norm = reportedProvider?.trim()?.lowercase(Locale.US).orEmpty()
    if (norm.startsWith("acp") || providerSessionId.startsWith("acp_")) {
        catalog.agents["acp"]?.forEach(::push)
        return models
    }
    val keys =
        when {
            engine == EngineKind.PLUGIN ->
                when {
                    norm.isNotEmpty() && !catalog.agents[norm].isNullOrEmpty() -> listOf(norm)
                    norm == "gemini" || norm == "gemini_cli" || providerSessionId.startsWith("gemini_") ->
                        listOf("gemini_cli", "zcode")
                    norm == "zcode" || providerSessionId.startsWith("sess_") ->
                        listOf("zcode", "gemini_cli")
                    norm == "grok" || norm == "grok_build" || providerSessionId.startsWith("grok_") ->
                        listOf("grok_build", "grok")
                    norm.isNotEmpty() -> listOf(norm)
                    else -> emptyList()
                }
            else -> engine.catalogKeys
        }
    for (key in keys) {
        val group = catalog.agents[key]
        if (!group.isNullOrEmpty()) {
            group.forEach(::push)
            if (engine == EngineKind.PLUGIN) break
        }
    }
    // Only Codex has a flat list that belongs to it.
    if (models.isEmpty() && engine == EngineKind.CODEX) catalog.flat.forEach(::push)
    return models
}

/**
 * Rewrites a model id into the form the picker compares by.
 *
 * The Host reports a session's model in its own spelling (`gpt-5-codex`),
 * sometimes with a `[1m]` context suffix (`claude-sonnet-5[1m]`) that the
 * catalog does not carry. Comparing raw strings would silently fail to match,
 * and the picker would offer "默认" instead of naming the model in use.
 */
fun normalizeModelId(value: String): String =
    value
        .trim()
        .removeSuffix("[1m]")
        .removeSuffix("[1M]")
        .removePrefix("models/")

/** Finds the catalog entry a reported model id refers to, if any. */
fun matchCatalogModel(models: List<CatalogModel>, reported: String?): CatalogModel? {
    val wanted = normalizeModelId(reported.orEmpty()).lowercase(Locale.US)
    if (wanted.isEmpty()) return null
    return models.firstOrNull { normalizeModelId(it.id).lowercase(Locale.US) == wanted }
}

/**
 * Context-window capacity when the Host did not report one.
 *
 * The phone never shows a percentage against a made-up denominator, so this is
 * only used to *size* a bar whose numerator the provider did report. Same
 * thresholds the console uses, so both agree on 1M / 2M / 200k.
 */
fun inferModelContextWindow(model: String?, provider: String?): Long {
    val m = model?.lowercase(Locale.US).orEmpty()
    if (m.contains("2m")) return 2_000_000
    if (
        m.contains("1m") ||
        m.startsWith("claude-opus-5") ||
        m.startsWith("claude-sonnet-5") ||
        m.startsWith("claude-fable-5") ||
        m.startsWith("claude-mythos-5") ||
        m.contains("anthropic.claude-sonnet-5") ||
        m.contains("gemini")
    ) {
        return 1_000_000
    }
    val p = provider?.lowercase(Locale.US).orEmpty()
    if (p.contains("antigravity") && !m.contains("claude") && !m.contains("gpt")) {
        return 1_000_000
    }
    if (
        m.isNotEmpty() ||
        p.contains("claude") ||
        p.contains("codex") ||
        p.contains("zcode") ||
        p.contains("antigravity") ||
        p.contains("grok")
    ) {
        return 200_000
    }
    return 0
}
