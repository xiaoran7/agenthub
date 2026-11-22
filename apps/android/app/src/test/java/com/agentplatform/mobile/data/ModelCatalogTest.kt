package com.agentplatform.mobile.data

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Engine catalog tests.
 *
 * The invariant under test is the one that cannot be verified on a device
 * without a live Host: an id from one engine's list is refused by another, so
 * the catalog must only ever offer the chosen engine's own models. Every test
 * here pins that rule or a consequence of it.
 */
class ModelCatalogTest {
    private fun catalog(
        data: List<JSONObject> = emptyList(),
        agents: Map<String, List<JSONObject>> = emptyMap(),
    ) = ModelCatalog.parse(
        JSONObject()
            .put("data", JSONArray().apply { data.forEach { put(it) } })
            .put(
                "agents",
                JSONObject().apply {
                    agents.forEach { (key, list) ->
                        put(key, JSONArray().apply { list.forEach { put(it) } })
                    }
                },
            ),
    )

    private fun entry(id: String, displayName: String? = null) =
        JSONObject().put("id", id).apply { if (displayName != null) put("display_name", displayName) }

    // ------------------------------------------------------------------ parse

    @Test
    fun `a catalog reads the flat list and every engine group`() {
        val parsed =
            catalog(
                data = listOf(entry("gpt-5-codex", "GPT-5 Codex")),
                agents =
                    mapOf(
                        "claude_code" to listOf(entry("claude-sonnet-5", "Sonnet 5")),
                        "codex" to listOf(entry("gpt-5-codex", "GPT-5 Codex")),
                        "zcode" to listOf(entry("gemini-3.8-flash", "Gemini 3.8 Flash")),
                    ),
            )

        assertEquals("GPT-5 Codex", parsed.flat.single().displayName)
        assertEquals(listOf("claude_code", "codex", "zcode"), parsed.agents.keys.sorted())
        assertFalse(parsed.isEmpty)
    }

    @Test
    fun `a display name is accepted under every spelling the host has used`() {
        val parsed =
            catalog(
                agents =
                    mapOf(
                        "codex" to
                            listOf(
                                JSONObject().put("id", "a").put("displayName", "camelCase"),
                                JSONObject().put("id", "b").put("display_name", "snake_case"),
                                JSONObject().put("id", "c").put("name", "plain"),
                                JSONObject().put("id", "d"),
                            ),
                    ),
            )

        val models = modelsForEngine(parsed, EngineKind.CODEX)
        assertEquals(listOf("camelCase", "snake_case", "plain", "d"), models.map { it.displayName })
    }

    @Test
    fun `an absent models object is an empty catalog rather than a crash`() {
        assertTrue(ModelCatalog.parse(null).isEmpty)
        assertTrue(ModelCatalog.parse(JSONObject()).isEmpty)
    }

    @Test
    fun `a malformed entry is skipped and does not take the rest of the list with it`() {
        val parsed =
            catalog(
                agents =
                    mapOf(
                        "codex" to
                            listOf(
                                JSONObject().put("display_name", "no id at all"),
                                entry("gpt-5-codex"),
                            ),
                    ),
            )

        assertEquals(listOf("gpt-5-codex"), modelsForEngine(parsed, EngineKind.CODEX).map { it.id })
    }

    @Test
    fun `an empty engine group is dropped so it does not read as advertised`() {
        val parsed = catalog(agents = mapOf("codex" to emptyList(), "zcode" to listOf(entry("m"))))
        assertNull(parsed.agents["codex"])
        assertEquals(listOf("zcode"), parsed.agents.keys.toList())
    }

    // ----------------------------------------------------------------- engine

    @Test
    fun `each engine reads only its own list`() {
        val parsed =
            catalog(
                agents =
                    mapOf(
                        "codex" to listOf(entry("gpt-5-codex")),
                        "claude_code" to listOf(entry("claude-sonnet-5")),
                        "zcode" to listOf(entry("gemini-3.8-flash")),
                        "antigravity" to listOf(entry("antigravity-model")),
                        "deepseek_harness" to listOf(entry("deepseek-reasoner")),
                        "grok_build" to listOf(entry("grok-4")),
                    ),
            )

        assertEquals(listOf("gpt-5-codex"), modelsForEngine(parsed, EngineKind.CODEX).map { it.id })
        assertEquals(listOf("claude-sonnet-5"), modelsForEngine(parsed, EngineKind.CLAUDE_CODE).map { it.id })
        assertEquals(listOf("gemini-3.8-flash"), modelsForEngine(parsed, EngineKind.PLUGIN, "zcode").map { it.id })
        assertEquals(listOf("antigravity-model"), modelsForEngine(parsed, EngineKind.ANTIGRAVITY).map { it.id })
        assertEquals(listOf("deepseek-reasoner"), modelsForEngine(parsed, EngineKind.DEEPSEEK_HARNESS).map { it.id })
        assertEquals(listOf("grok-4"), modelsForEngine(parsed, EngineKind.PLUGIN, "grok_build").map { it.id })
    }

    @Test
    fun `an engine with no advertised list offers nothing instead of another engine's models`() {
        val parsed = catalog(agents = mapOf("codex" to listOf(entry("gpt-5-codex"))))

        assertTrue(modelsForEngine(parsed, EngineKind.CLAUDE_CODE).isEmpty())
        assertTrue(modelsForEngine(parsed, EngineKind.PLUGIN, "zcode").isEmpty())
        // Codex still has its own.
        assertEquals(1, modelsForEngine(parsed, EngineKind.CODEX).size)
    }

    @Test
    fun `the legacy claude key still feeds claude_code`() {
        val parsed = catalog(agents = mapOf("claude" to listOf(entry("claude-sonnet-5"))))
        assertEquals(listOf("claude-sonnet-5"), modelsForEngine(parsed, EngineKind.CLAUDE_CODE).map { it.id })
    }

    @Test
    fun `the flat list is offered for codex and nowhere else`() {
        val parsed = catalog(data = listOf(entry("gpt-5-codex")))

        assertEquals(listOf("gpt-5-codex"), modelsForEngine(parsed, EngineKind.CODEX).map { it.id })
        assertTrue(modelsForEngine(parsed, EngineKind.CLAUDE_CODE).isEmpty())
        assertTrue(modelsForEngine(parsed, EngineKind.PLUGIN, "zcode").isEmpty())
    }

    @Test
    fun `an engine list wins over the flat list for codex`() {
        val parsed =
            catalog(
                data = listOf(entry("stale-flat")),
                agents = mapOf("codex" to listOf(entry("gpt-5-codex"))),
            )
        assertEquals(listOf("gpt-5-codex"), modelsForEngine(parsed, EngineKind.CODEX).map { it.id })
    }

    @Test
    fun `a repeated id across keys is offered once`() {
        val parsed =
            catalog(
                agents =
                    mapOf(
                        "claude_code" to listOf(entry("claude-sonnet-5", "Sonnet 5")),
                        "claude" to listOf(entry("claude-sonnet-5", "Sonnet 5 again")),
                    ),
            )

        val models = modelsForEngine(parsed, EngineKind.CLAUDE_CODE)
        assertEquals(listOf("claude-sonnet-5"), models.map { it.id })
    }

    @Test
    fun `the synthetic placeholder is never offered`() {
        val parsed = catalog(agents = mapOf("codex" to listOf(entry("<synthetic>"))))
        assertTrue(modelsForEngine(parsed, EngineKind.CODEX).isEmpty())
    }

    // ---------------------------------------------------------- classification

    @Test
    fun `a reported provider names the engine whatever the session id looks like`() {
        assertEquals(EngineKind.CLAUDE_CODE, engineOfSession("sess_abc", "claude_code"))
        assertEquals(EngineKind.CODEX, engineOfSession("claude_abc", "codex"))
        assertEquals(EngineKind.PLUGIN, engineOfSession("claude_abc", "gemini_cli"))
        assertEquals(EngineKind.ANTIGRAVITY, engineOfSession("sess_abc", "antigravity"))
        assertEquals(EngineKind.DEEPSEEK_HARNESS, engineOfSession("sess_abc", "deepseek_harness"))
        assertEquals(EngineKind.PLUGIN, engineOfSession("sess_abc", "grok_build"))
    }

    @Test
    fun `claude desktop is its own engine and reads its own catalog`() {
        assertEquals(
            EngineKind.CLAUDE_DESKTOP,
            engineOfSession("sess_abc", "claude_desktop"),
        )
        assertEquals(listOf("claude_desktop"), EngineKind.CLAUDE_DESKTOP.catalogKeys)

        val catalog =
            ModelCatalog(
                flat = emptyList(),
                agents =
                    mapOf(
                        "claude_desktop" to listOf(CatalogModel("desk-1", "Desk 1")),
                        "claude_code" to listOf(CatalogModel("code-1", "Code 1")),
                    ),
            )
        assertEquals(
            listOf(CatalogModel("desk-1", "Desk 1")),
            modelsForEngine(catalog, EngineKind.CLAUDE_DESKTOP),
        )
    }

    @Test
    fun `zcode plugin reads both the keys the runner publishes for it`() {
        val catalog =
            ModelCatalog(
                flat = emptyList(),
                agents = mapOf("gemini_cli" to listOf(CatalogModel("gemini-3.8", "Gemini 3.8"))),
            )
        assertEquals(
            listOf(CatalogModel("gemini-3.8", "Gemini 3.8")),
            modelsForEngine(catalog, EngineKind.PLUGIN, "zcode"),
        )
    }

    @Test
    fun `non-native valid plugin slugs classify as PLUGIN`() {
        assertEquals(EngineKind.PLUGIN, engineOfSession("sess_abc", "zcode"))
        assertEquals(EngineKind.PLUGIN, engineOfSession("sess_abc", "gemini_cli"))
        assertEquals(EngineKind.PLUGIN, engineOfSession("opaque", "custom_plugin"))
    }

    @Test
    fun `a claude_desktop session is its own engine whatever its id says`() {
        assertEquals(EngineKind.CLAUDE_DESKTOP, engineOfSession("claude_abc", "claude_desktop"))
        assertEquals(EngineKind.CLAUDE_CODE, engineOfSession("claude_abc", null))
        assertEquals(EngineKind.CLAUDE_CODE, engineOfSession("claude_desktop_abc", null))
    }

    @Test
    fun `an undescribed session falls back to its id prefix`() {
        assertEquals(EngineKind.PLUGIN, engineOfSession("sess_abc", null))
        assertEquals(EngineKind.PLUGIN, engineOfSession("gemini_abc", null))
        assertEquals(EngineKind.CLAUDE_CODE, engineOfSession("claude_abc", null))
        assertEquals(EngineKind.ANTIGRAVITY, engineOfSession("antigravity_abc", null))
        assertEquals(EngineKind.DEEPSEEK_HARNESS, engineOfSession("deepseek_abc", null))
        assertEquals(EngineKind.PLUGIN, engineOfSession("grok_abc", null))
        assertEquals(EngineKind.CODEX, engineOfSession("opaque", null))
    }

    // ------------------------------------------------------------ matching ids

    @Test
    fun `a context suffix on the reported model still matches the catalog entry`() {
        val models = listOf(CatalogModel("claude-sonnet-5", "Sonnet 5"))
        assertEquals(
            "claude-sonnet-5",
            matchCatalogModel(models, "claude-sonnet-5[1m]")?.id,
        )
    }

    @Test
    fun `a models prefix on either side is ignored when matching`() {
        val models = listOf(CatalogModel("models/gpt-5-codex", "GPT-5 Codex"))
        assertEquals("models/gpt-5-codex", matchCatalogModel(models, "gpt-5-codex")?.id)
    }

    @Test
    fun `a model the catalog does not carry matches nothing`() {
        val models = listOf(CatalogModel("claude-sonnet-5", "Sonnet 5"))
        assertNull(matchCatalogModel(models, "totally-other"))
        assertNull(matchCatalogModel(models, null))
        assertNull(matchCatalogModel(models, "  "))
    }

    // ------------------------------------------------------- context inference

    @Test
    fun `a context window is inferred from the model name`() {
        assertEquals(2_000_000, inferModelContextWindow("claude-sonnet-5[2m]", null))
        assertEquals(1_000_000, inferModelContextWindow("gemini-3.8-flash", null))
        assertEquals(1_000_000, inferModelContextWindow("claude-sonnet-5", null))
        assertEquals(200_000, inferModelContextWindow("gpt-5-codex", null))
    }

    @Test
    fun `a provider alone still yields a window but an unknown model yields none`() {
        assertEquals(200_000, inferModelContextWindow(null, "claude_code"))
        assertEquals(200_000, inferModelContextWindow(null, "zcode"))
        assertEquals(0, inferModelContextWindow(null, "something-else"))
        assertEquals(0, inferModelContextWindow("", ""))
    }
}
