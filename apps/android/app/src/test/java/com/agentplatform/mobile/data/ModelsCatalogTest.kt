package com.agentplatform.mobile.data

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Wire tests for the fields the v2.6.3 catalog work added to `hello`.
 *
 * `models` and the token fields on a discovery candidate are typed `unknown` in
 * the contract, so nothing upstream guarantees their shape. These tests pin
 * what the phone accepts and, more importantly, what it refuses to guess at:
 * a missing or malformed field has to degrade to "no choice" rather than
 * crashing a screen or sending a model id the Runner will reject.
 */
class ModelsCatalogTest {
    private fun helloWith(json: JSONObject.() -> Unit) =
        HostHello.parse(
            JSONObject()
                .put("host_generation", "gen-1")
                .put("workspaces", JSONArray())
                .apply(json),
        )

    // ------------------------------------------------------------------ models

    @Test
    fun `hello models parse into a catalog and are reachable from the host`() {
        val host =
            HostDevice.parse(
                JSONObject()
                    .put("runner_id", "runner-a")
                    .put("online", true)
                    .put(
                        "hello",
                        JSONObject()
                            .put("host_generation", "gen-1")
                            .put("workspaces", JSONArray())
                            .put(
                                "models",
                                JSONObject()
                                    .put(
                                        "agents",
                                        JSONObject()
                                            .put(
                                                "codex",
                                                JSONArray()
                                                    .put(
                                                        JSONObject()
                                                            .put("id", "gpt-5-codex")
                                                            .put("display_name", "GPT-5 Codex"),
                                                    ),
                                            ),
                                    ),
                            ),
                    ),
            )

        val models = modelsForEngine(host!!.models, EngineKind.CODEX)
        assertEquals(listOf("gpt-5-codex"), models.map { it.id })
        assertEquals(listOf("GPT-5 Codex"), models.map { it.displayName })
    }

    @Test
    fun `a host without models has an empty catalog`() {
        val host = HostDevice.parse(JSONObject().put("runner_id", "runner-a").put("online", true))!!

        assertTrue(host.models.isEmpty)
        assertTrue(modelsForEngine(host.models, EngineKind.CODEX).isEmpty())
    }

    @Test
    fun `a malformed models object degrades to an empty catalog`() {
        // A runner that predates the field sends nothing; one that sends a
        // different shape must not take the session screen down with it.
        val hello = helloWith { put("models", JSONArray().put("not-an-object")) }

        assertTrue(hello?.models?.isEmpty == true)
    }

    // ------------------------------------------------------- discovery tokens

    @Test
    fun `discovery candidates carry the measured context window and usage`() {
        val hello =
            helloWith {
                put(
                    "discovery",
                    JSONObject()
                        .put(
                            "candidates",
                            JSONArray()
                                .put(
                                    JSONObject()
                                        .put("provider_session_id", "claude_abc")
                                        .put("workspace_path", "/w/one")
                                        .put("model", "claude-sonnet-5")
                                        .put("context_window_tokens", 200_000)
                                        .put("context_used_tokens", 61_000)
                                        .put("prompt_tokens", 5_000),
                                ),
                        ),
                )
            }

        val candidate = hello?.candidates?.single()
        assertEquals(200_000L, candidate?.contextWindowTokens)
        assertEquals(61_000L, candidate?.contextUsedTokens)
        // A measured window wins over anything the model name would suggest.
        assertEquals(200_000L, candidate?.inferredContextWindow)
    }

    @Test
    fun `a candidate without a measured window infers one from its model`() {
        val hello =
            helloWith {
                put(
                    "discovery",
                    JSONObject()
                        .put(
                            "candidates",
                            JSONArray()
                                .put(
                                    JSONObject()
                                        .put("provider_session_id", "gemini_abc")
                                        .put("workspace_path", "/w/one")
                                        .put("model", "gemini-3.8-flash")
                                        .put("prompt_tokens", 4_000),
                                ),
                        ),
                )
            }

        val candidate = hello?.candidates?.single()
        assertEquals(1_000_000L, candidate?.inferredContextWindow)
    }

    @Test
    fun `a candidate with no model and no measured window reports an unknown one`() {
        val hello =
            helloWith {
                put(
                    "discovery",
                    JSONObject()
                        .put(
                            "candidates",
                            JSONArray()
                                .put(
                                    JSONObject()
                                        .put("provider_session_id", "claude_abc")
                                        .put("workspace_path", "/w/one"),
                                ),
                        ),
                )
            }

        assertEquals(0L, hello?.candidates?.single()?.inferredContextWindow)
    }

    @Test
    fun `a candidate names its engine so only its own models are offered`() {
        val hello =
            helloWith {
                put(
                    "discovery",
                    JSONObject()
                        .put(
                            "candidates",
                            JSONArray()
                                .put(
                                    JSONObject()
                                        .put("provider_session_id", "opaque-id")
                                        .put("workspace_path", "/w/one")
                                        .put("provider", "antigravity"),
                                ),
                        ),
                )
            }

        assertEquals(EngineKind.ANTIGRAVITY, hello?.candidates?.single()?.engine)
    }

    // -------------------------------------------------------------- lookups

    @Test
    fun `a snapshot finds the discovery record for one provider session`() {
        val snapshot =
            Snapshot.parse(
                JSONObject()
                    .put(
                        "runners",
                        JSONArray()
                            .put(
                                JSONObject()
                                    .put("runner_id", "runner-a")
                                    .put("online", true)
                                    .put(
                                        "hello",
                                        JSONObject()
                                            .put("host_generation", "gen-1")
                                            .put("workspaces", JSONArray().put("/w/one"))
                                            .put(
                                                "discovery",
                                                JSONObject()
                                                    .put(
                                                        "candidates",
                                                        JSONArray()
                                                            .put(
                                                                JSONObject()
                                                                    .put(
                                                                        "provider_session_id",
                                                                        "claude_abc",
                                                                    )
                                                                    .put("workspace_path", "/w/one")
                                                                    .put("title", "修登录"),
                                                            ),
                                                    ),
                                            ),
                                    ),
                            ),
                    )
                    .put("sessions", JSONArray()),
            )

        assertEquals("修登录", snapshot.candidate("runner-a", "claude_abc")?.title)
        assertNull(snapshot.candidate("runner-a", "claude_missing"))
        assertNull(snapshot.candidate("runner-missing", "claude_abc"))
    }

    @Test
    fun `a snapshot with no hello has no candidate for any session`() {
        val snapshot =
            Snapshot.parse(
                JSONObject()
                    .put("runners", JSONArray().put(JSONObject().put("runner_id", "runner-a")))
                    .put("sessions", JSONArray()),
            )

        assertNull(snapshot.candidate("runner-a", "anything"))
        assertFalse(snapshot.host("runner-a")?.models?.isEmpty == false)
    }
}
