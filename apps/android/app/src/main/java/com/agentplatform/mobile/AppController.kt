package com.agentplatform.mobile

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.agentplatform.mobile.data.AgentGateway
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.DeviceRecord
import com.agentplatform.mobile.data.SecureConfigStore
import com.agentplatform.mobile.data.SessionRecord
import com.agentplatform.mobile.data.describeGatewayError
import java.io.Closeable
import java.time.Instant
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicLong

data class AppUiState(
    val config: AppConfig?,
    val devices: List<DeviceRecord> = emptyList(),
    val sessions: List<SessionRecord> = emptyList(),
    val selectedSession: SessionRecord? = null,
    val timeline: List<SessionRecord> = emptyList(),
    val loading: Boolean = false,
    val timelineLoading: Boolean = false,
    val error: String? = null,
    val timelineError: String? = null,
    val lastUpdatedAt: Instant? = null,
)

class AppController(
    private val configStore: SecureConfigStore,
) : Closeable {
    private val executor = Executors.newSingleThreadExecutor()
    private val mainHandler = Handler(Looper.getMainLooper())
    private val requestGeneration = AtomicLong()

    var state by mutableStateOf(AppUiState(config = configStore.load()))
        private set

    init {
        if (state.config != null) {
            refresh()
        }
    }

    fun refresh() {
        val config = state.config ?: return
        val request = requestGeneration.incrementAndGet()
        state = state.copy(loading = true, error = null)
        executor.execute {
            runCatching { AgentGateway(config).fetchSnapshot() }
                .onSuccess { snapshot ->
                    mainHandler.post {
                        if (request != requestGeneration.get()) return@post
                        val selectedKey = state.selectedSession?.key
                        val selected =
                            selectedKey?.let { key ->
                                snapshot.sessions.firstOrNull { it.key == key }
                            }
                        state =
                            state.copy(
                                devices = snapshot.devices,
                                sessions = snapshot.sessions,
                                selectedSession = selected,
                                loading = false,
                                error = null,
                                lastUpdatedAt = Instant.now(),
                            )
                        if (selected != null) {
                            loadTimeline(selected)
                        }
                    }
                }
                .onFailure { error ->
                    mainHandler.post {
                        if (request != requestGeneration.get()) return@post
                        state =
                            state.copy(
                                loading = false,
                                error = describeGatewayError(error),
                            )
                    }
                }
        }
    }

    fun saveConfig(config: AppConfig): String? {
        val normalized = config.normalized()
        normalized.validationError()?.let { return it }
        configStore.save(normalized)
        requestGeneration.incrementAndGet()
        state =
            AppUiState(
                config = normalized,
                loading = true,
            )
        refresh()
        return null
    }

    fun selectSession(session: SessionRecord?) {
        state =
            state.copy(
                selectedSession = session,
                timeline = emptyList(),
                timelineError = null,
                timelineLoading = session != null,
            )
        if (session != null) {
            loadTimeline(session)
        }
    }

    fun clearConfig() {
        requestGeneration.incrementAndGet()
        configStore.clear()
        state = AppUiState(config = null)
    }

    private fun loadTimeline(session: SessionRecord) {
        val config = state.config ?: return
        val sessionKey = session.key
        state = state.copy(timelineLoading = true, timelineError = null)
        executor.execute {
            runCatching { AgentGateway(config).fetchTimeline(session) }
                .onSuccess { events ->
                    mainHandler.post {
                        if (state.selectedSession?.key != sessionKey) return@post
                        state =
                            state.copy(
                                timeline = events,
                                timelineLoading = false,
                                timelineError = null,
                            )
                    }
                }
                .onFailure { error ->
                    mainHandler.post {
                        if (state.selectedSession?.key != sessionKey) return@post
                        state =
                            state.copy(
                                timelineLoading = false,
                                timelineError = describeGatewayError(error),
                            )
                    }
                }
        }
    }

    override fun close() {
        requestGeneration.incrementAndGet()
        executor.shutdownNow()
    }
}
