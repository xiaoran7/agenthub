package com.agentplatform.mobile

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.agentplatform.mobile.data.AccountView
import com.agentplatform.mobile.data.AgentGateway
import com.agentplatform.mobile.data.AgentPlugin
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.AppearanceStore
import com.agentplatform.mobile.data.AppearanceTheme
import com.agentplatform.mobile.data.CommandLedger
import com.agentplatform.mobile.data.CommandReceipt
import com.agentplatform.mobile.data.DeviceBinding
import com.agentplatform.mobile.data.GatewayException
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.SecureConfigStore
import com.agentplatform.mobile.data.ServerAddressStore
import com.agentplatform.mobile.data.Snapshot
import com.agentplatform.mobile.data.describeGatewayError
import com.agentplatform.mobile.data.mergeHostHello
import com.agentplatform.mobile.data.optNullableString
import java.io.Closeable
import java.time.Instant
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * Everything the app shell needs to render: who is signed in, which Gateway,
 * and the fleet as of the last successful poll.
 */
data class AppUiState(
    val config: AppConfig? = null,
    val account: AccountView? = null,
    val snapshot: Snapshot = Snapshot.Empty,
    val plugins: List<AgentPlugin>? = null,
    val pluginRunners: List<com.agentplatform.mobile.data.RunnerPluginSnapshot> = emptyList(),
    /** First load of the fleet, before any snapshot is available. */
    val loading: Boolean = false,
    /** A sign-in or profile fetch is in flight. */
    val authenticating: Boolean = false,
    val error: String? = null,
    val loggedOut: Boolean = false,
    val lastUpdatedAt: Instant? = null,
    /** The appearance the user picked; survives a restart. */
    val appearance: AppearanceTheme = AppearanceTheme.SYSTEM,
) {
    val needsLogin: Boolean
        get() = config != null && account == null && !loggedOut && error == null

    val signedIn: Boolean
        get() = config != null && account != null

    val enabledPluginIds: Set<String>
        get() = plugins?.filter { it.enabled }?.mapTo(HashSet()) { it.id }.orEmpty()
}

/**
 * Sanitizes an [AppConfig] update before saving so credentials from one
 * Gateway deployment are never sent cross-origin when switching servers.
 */
internal fun sanitizeDeploymentChange(previous: AppConfig?, incoming: AppConfig): AppConfig {
    val normIncoming = incoming.normalized()
    val normPrev = previous?.normalized() ?: return normIncoming
    if (normPrev.gatewayBase == normIncoming.gatewayBase) return normIncoming
    return normIncoming.copy(
        sessionCookie = if (normIncoming.sessionCookie == normPrev.sessionCookie) "" else normIncoming.sessionCookie,
        accessToken = if (normIncoming.accessToken == normPrev.accessToken) "" else normIncoming.accessToken,
    )
}

class AppController(
    private val configStore: SecureConfigStore,
    private val appearanceStore: AppearanceStore? = null,
    private val serverAddressStore: ServerAddressStore? = null,
) : Closeable {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val ledger = CommandLedger()
    private val pollGeneration = AtomicLong(0L)
    private val sessionEpoch = AtomicLong(0L)
    private val configMutex = Mutex()
    private val lastPersistedEpoch = AtomicLong(0L)
    private val pluginsRequestSeq = AtomicLong(0L)
    private var authJob: Job? = null

    private var clientConfig: AppConfig? = null
    private var client: AgentGateway? = null
    private var configPendingSave = false
    private var queuedPrompts by mutableStateOf<Map<String, com.agentplatform.mobile.data.QueuedPromptDraft>>(emptyMap())

    internal fun queuedPromptFor(sessionKey: String): com.agentplatform.mobile.data.QueuedPromptDraft? =
        queuedPrompts[sessionKey]

    internal fun setQueuedPromptFor(
        sessionKey: String,
        draft: com.agentplatform.mobile.data.QueuedPromptDraft?,
    ) {
        queuedPrompts =
            if (draft == null) {
                queuedPrompts - sessionKey
            } else {
                queuedPrompts + (sessionKey to draft)
            }
    }

    private val helloFetchedAtMs = HashMap<String, Long>()
    private val helloInFlight = HashSet<String>()
    private var pluginsFetchedAtMs = 0L
    private var activePluginsFetchSeq = 0L

    var state by mutableStateOf(
        AppUiState(
            config = configStore.load(),
            appearance = appearanceStore?.load() ?: AppearanceTheme.SYSTEM,
        ),
    )
        private set

    val savedGatewayBase: String
        get() = serverAddressStore?.loadGatewayBase()?.takeIf { it.isNotBlank() }
            ?: state.config?.gatewayBase.orEmpty()

    val savedEmail: String
        get() = serverAddressStore?.loadEmail().orEmpty()

    val savedDevUser: String
        get() = serverAddressStore?.loadDevUser()?.takeIf { it.isNotBlank() }
            ?: state.config?.developmentUserId.orEmpty()

    fun rememberServerAddress(gatewayBase: String, email: String, devUser: String = "") {
        if (gatewayBase.isNotBlank()) serverAddressStore?.saveGatewayBase(gatewayBase)
        if (email.isNotBlank()) serverAddressStore?.saveEmail(email)
        if (devUser.isNotBlank()) serverAddressStore?.saveDevUser(devUser)
    }

    fun updateHostDevice(updated: HostDevice) {
        val current = state.snapshot
        val existing = current.hosts
        val idx = existing.indexOfFirst { it.runnerId == updated.runnerId }
        val newHosts =
            if (idx >= 0) {
                val prev = existing[idx]
                val merged = updated.copy(hello = mergeHostHello(updated.hello, prev.hello))
                existing.toMutableList().apply { set(idx, merged) }
            } else {
                existing + updated
            }
        state = state.copy(snapshot = current.copy(hosts = newHosts))
    }

    suspend fun loadRunnerHello(runnerId: String, force: Boolean = true): HostDevice? {
        val gw = gateway ?: return null
        val epoch = sessionEpoch.get()
        val now = System.currentTimeMillis()
        if (!force) {
            val last = helloFetchedAtMs[runnerId] ?: 0L
            if (now - last < HELLO_TTL_MS) return state.snapshot.host(runnerId)
        }
        if (!helloInFlight.add(runnerId)) return state.snapshot.host(runnerId)
        return try {
            val device = withContext(Dispatchers.IO) { gw.fetchRunnerHello(runnerId) }
            if (epoch != sessionEpoch.get()) return null
            device?.also {
                helloFetchedAtMs[runnerId] = System.currentTimeMillis()
                updateHostDevice(it)
            }
        } finally {
            if (epoch == sessionEpoch.get()) {
                helloInFlight.remove(runnerId)
            }
        }
    }

    suspend fun loadPlugins(force: Boolean = false): List<AgentPlugin>? {
        val gw = gateway ?: return null
        val epoch = sessionEpoch.get()
        val now = System.currentTimeMillis()
        if (!force && state.plugins != null && now - pluginsFetchedAtMs < PLUGINS_TTL_MS) {
            return state.plugins
        }
        if (!force && activePluginsFetchSeq != 0L && activePluginsFetchSeq == pluginsRequestSeq.get()) {
            return state.plugins
        }
        val seq = pluginsRequestSeq.incrementAndGet()
        activePluginsFetchSeq = seq
        return try {
            val resp = runCatching { withContext(Dispatchers.IO) { gw.fetchPluginsResponse() } }.getOrNull()
            if (epoch != sessionEpoch.get() || seq != pluginsRequestSeq.get()) return state.plugins
            resp?.also {
                pluginsFetchedAtMs = System.currentTimeMillis()
                state = state.copy(plugins = it.plugins, pluginRunners = it.runners)
            }?.plugins
        } finally {
            if (epoch == sessionEpoch.get() && activePluginsFetchSeq == seq) {
                activePluginsFetchSeq = 0L
            }
        }
    }

    fun performPluginAction(
        runnerId: String,
        action: String,
        pluginId: String? = null,
        enabled: Boolean? = null,
        registryUrl: String? = null,
        onResult: (Result<List<AgentPlugin>>) -> Unit = {},
    ) {
        val gw = gateway
        if (gw == null) {
            onResult(Result.failure(GatewayException(0, "not_configured")))
            return
        }
        val epoch = sessionEpoch.get()
        scope.launch {
            val res =
                runCatching {
                    withContext(Dispatchers.IO) {
                        gw.performPluginAction(
                            runnerId = runnerId,
                            action = action,
                            pluginId = pluginId,
                            enabled = enabled,
                            registryUrl = registryUrl,
                        )
                    }
                }
            if (res.exceptionOrNull() is CancellationException || epoch != sessionEpoch.get()) return@launch
            res.onSuccess { list ->
                pluginsRequestSeq.incrementAndGet()
                pluginsFetchedAtMs = System.currentTimeMillis()
                val nextRunners =
                    if (state.pluginRunners.any { it.runnerId == runnerId }) {
                        state.pluginRunners.map { snap ->
                            if (snap.runnerId == runnerId) snap.copy(plugins = list) else snap
                        }
                    } else {
                        state.pluginRunners +
                            com.agentplatform.mobile.data.RunnerPluginSnapshot(
                                runnerId = runnerId,
                                deviceName = state.snapshot.host(runnerId)?.hello?.deviceName,
                                online = state.snapshot.host(runnerId)?.online ?: true,
                                registryUrl = null,
                                updatedAt = null,
                                plugins = list,
                            )
                    }
                val byId = linkedMapOf<String, AgentPlugin>()
                for (r in nextRunners) {
                    for (p in r.plugins) {
                        val prev = byId[p.id]
                        if (prev == null || (!prev.installed && p.installed) || (!prev.enabled && p.enabled)) {
                            byId[p.id] = p
                        }
                    }
                }
                state = state.copy(pluginRunners = nextRunners, plugins = byId.values.toList())
                loadPlugins(force = true)
            }
            if (epoch != sessionEpoch.get()) return@launch
            onResult(res)
        }
    }

    fun togglePlugin(
        pluginId: String,
        enabled: Boolean,
        onResult: (Result<List<AgentPlugin>>) -> Unit = {},
    ) {
        val gw = gateway
        if (gw == null) {
            onResult(Result.failure(GatewayException(0, "not_configured")))
            return
        }
        val epoch = sessionEpoch.get()
        scope.launch {
            val raw = runCatching { withContext(Dispatchers.IO) { gw.togglePlugin(pluginId, enabled) } }
            if (raw.exceptionOrNull() is CancellationException || epoch != sessionEpoch.get()) return@launch
            val res =
                raw.map { plugin ->
                    val currentPlugins = state.plugins.orEmpty()
                    if (plugin != null) {
                        if (currentPlugins.any { it.id == plugin.id }) {
                            currentPlugins.map { if (it.id == plugin.id) plugin else it }
                        } else {
                            currentPlugins + plugin
                        }
                    } else {
                        currentPlugins.map {
                            if (it.id == pluginId) it.copy(enabled = enabled) else it
                        }
                    }
                }
            res.onSuccess { list ->
                pluginsRequestSeq.incrementAndGet()
                pluginsFetchedAtMs = System.currentTimeMillis()
                state = state.copy(plugins = list)
            }
            onResult(res)
        }
    }

    fun createBindingCode(onResult: (Result<DeviceBinding>) -> Unit) {
        val gw = gateway
        if (gw == null) {
            onResult(Result.failure(GatewayException(0, "not_configured")))
            return
        }
        val epoch = sessionEpoch.get()
        scope.launch {
            val res = runCatching { withContext(Dispatchers.IO) { gw.createDeviceBinding() } }
            if (res.exceptionOrNull() is CancellationException || epoch != sessionEpoch.get()) return@launch
            onResult(res)
        }
    }

    fun revokeHostDevice(runnerId: String, onResult: (Result<Unit>) -> Unit) {
        val gw = gateway
        if (gw == null) {
            onResult(Result.failure(GatewayException(0, "not_configured")))
            return
        }
        val epoch = sessionEpoch.get()
        scope.launch {
            val res = runCatching { withContext(Dispatchers.IO) { gw.revokeDevice(runnerId) } }
            if (res.exceptionOrNull() is CancellationException || epoch != sessionEpoch.get()) return@launch
            res.onSuccess {
                helloFetchedAtMs.remove(runnerId)
                refresh()
            }
            onResult(res)
        }
    }

    val gateway: AgentGateway?
        get() {
            if (configPendingSave) return null
            val config = state.config ?: return null
            val held = client
            if (held != null && clientConfig == config) return held
            return AgentGateway(config).also {
                clientConfig = config
                client = it
            }
        }

    init {
        val config = state.config
        if (config != null && (config.hasCredential || config.developmentUserId.isNotBlank())) {
            signInWithSavedCredential()
        }
    }

    private suspend fun persistConfig(config: AppConfig, epoch: Long): Result<Boolean> =
        configMutex.withLock {
            if (epoch != sessionEpoch.get()) return@withLock Result.success(false)
            withContext(Dispatchers.IO) {
                runCatching {
                    if (epoch != sessionEpoch.get()) return@runCatching false
                    configStore.save(config)
                    lastPersistedEpoch.set(epoch)
                    true
                }
            }
        }

    private fun enqueueClearStore(clearEpoch: Long) {
        scope.launch {
            configMutex.withLock {
                if (clearEpoch < lastPersistedEpoch.get()) return@withLock
                withContext(Dispatchers.IO) {
                    runCatching { configStore.clear() }
                    Unit
                }
            }
        }
    }

    private fun clearTransientSessionCaches() {
        client = null
        clientConfig = null
        configPendingSave = false
        queuedPrompts = emptyMap()
        helloFetchedAtMs.clear()
        helloInFlight.clear()
        pluginsFetchedAtMs = 0L
        activePluginsFetchSeq = 0L
        pluginsRequestSeq.incrementAndGet()
    }

    private fun resetSessionState(clearStore: Boolean = false) {
        val epoch = sessionEpoch.incrementAndGet()
        pollGeneration.incrementAndGet()
        clearTransientSessionCaches()
        state =
            state.copy(
                config = if (clearStore) null else state.config,
                snapshot = Snapshot.Empty,
                plugins = null,
                pluginRunners = emptyList(),
                lastUpdatedAt = null,
            )
        if (clearStore) {
            enqueueClearStore(epoch)
        }
    }

    private suspend fun probeSavedCredential(config: AppConfig, epoch: Long) {
        val probeClient = AgentGateway(config)
        val res = runCatching { withContext(Dispatchers.IO) { probeClient.account() } }
        if (res.exceptionOrNull() is CancellationException || epoch != sessionEpoch.get()) return
        res
            .onSuccess { account ->
                state = state.copy(account = account, authenticating = false, error = null)
                refresh()
            }
            .onFailure { failure ->
                if ((failure as? GatewayException)?.status == 401) {
                    resetSessionState(clearStore = true)
                    state =
                        state.copy(
                            config = null,
                            account = null,
                            authenticating = false,
                            loading = false,
                            error = describeGatewayError(failure),
                        )
                } else {
                    state = state.copy(authenticating = false, error = describeGatewayError(failure))
                }
            }
    }

    fun signInWithSavedCredential() {
        val config = state.config ?: return
        authJob?.cancel()
        val epoch = sessionEpoch.get()
        state = state.copy(authenticating = true, error = null, loggedOut = false)
        authJob = scope.launch { probeSavedCredential(config, epoch) }
    }

    fun login(email: String, password: String, gatewayBase: String) {
        serverAddressStore?.saveGatewayBase(gatewayBase)
        serverAddressStore?.saveEmail(email)
        val draft = AppConfig(gatewayBase = gatewayBase)
        draft.gatewayError()?.let { message ->
            state = state.copy(error = message)
            return
        }
        val loginClient = AgentGateway(draft.normalized())
        authJob?.cancel()
        val epoch = sessionEpoch.incrementAndGet()
        pollGeneration.incrementAndGet()
        client = null
        clientConfig = null
        configPendingSave = false
        state = state.copy(authenticating = true, error = null, loggedOut = false)
        authJob =
            scope.launch {
                val res = runCatching { withContext(Dispatchers.IO) { loginClient.login(email, password) } }
                if (res.exceptionOrNull() is CancellationException || epoch != sessionEpoch.get()) return@launch
                res
                    .onSuccess { result ->
                        val saved = draft.normalized().copy(sessionCookie = result.sessionCookie)
                        val saveRes = persistConfig(saved, epoch)
                        if (saveRes.exceptionOrNull() is CancellationException || epoch != sessionEpoch.get()) {
                            return@launch
                        }
                        if (saveRes.isFailure) {
                            state = state.copy(authenticating = false, error = STORAGE_SAVE_ERROR)
                            return@launch
                        }
                        if (saveRes.getOrNull() != true) return@launch
                        clearTransientSessionCaches()
                        state =
                            state.copy(
                                config = saved,
                                account = result.account,
                                snapshot = Snapshot.Empty,
                                plugins = null,
                                pluginRunners = emptyList(),
                                lastUpdatedAt = null,
                                authenticating = false,
                                error = null,
                                loggedOut = false,
                            )
                        refresh()
                    }
                    .onFailure { failure ->
                        state = state.copy(authenticating = false, error = describeGatewayError(failure))
                    }
            }
    }

    fun logout(errorMessage: String? = null) {
        val config = state.config
        authJob?.cancel()
        authJob = null
        resetSessionState(clearStore = true)
        if (config != null && config.sessionCookie.isNotBlank()) {
            val logoutClient = AgentGateway(config)
            scope.launch { withContext(Dispatchers.IO) { logoutClient.logout() } }
        }
        state =
            state.copy(
                config = null,
                account = null,
                authenticating = false,
                loading = false,
                error = errorMessage,
                loggedOut = true,
            )
    }

    /** Saves Gateway address and credentials, stripping cross-origin cookies on server switch. */
    fun saveConfig(config: AppConfig): String? {
        val sanitized = sanitizeDeploymentChange(state.config, config)
        val movedDeployment =
            state.config != null &&
                state.config?.normalized()?.gatewayBase != sanitized.gatewayBase
        if (movedDeployment && !sanitized.hasCredential && sanitized.developmentUserId.isBlank()) {
            sanitized.gatewayError()?.let { return it }
            serverAddressStore?.saveGatewayBase(sanitized.gatewayBase)
            authJob?.cancel()
            authJob = null
            resetSessionState(clearStore = true)
            state =
                state.copy(
                    config = null,
                    account = null,
                    authenticating = false,
                    loading = false,
                    error = null,
                    loggedOut = true,
                )
            return null
        }
        sanitized.validationError()?.let { return it }
        serverAddressStore?.saveGatewayBase(sanitized.gatewayBase)
        if (sanitized.developmentUserId.isNotBlank()) {
            serverAddressStore?.saveDevUser(sanitized.developmentUserId)
        }
        authJob?.cancel()
        val epoch = sessionEpoch.incrementAndGet()
        pollGeneration.incrementAndGet()
        client = null
        clientConfig = null
        if (movedDeployment) {
            clearTransientSessionCaches()
        }
        configPendingSave = true
        state =
            state.copy(
                config = if (movedDeployment) null else state.config,
                account = if (movedDeployment) null else state.account,
                snapshot = Snapshot.Empty,
                plugins = if (movedDeployment) null else state.plugins,
                pluginRunners = if (movedDeployment) emptyList() else state.pluginRunners,
                lastUpdatedAt = if (movedDeployment) null else state.lastUpdatedAt,
                authenticating = true,
                error = null,
                loggedOut = false,
            )
        authJob =
            scope.launch {
                val saveRes =
                    try {
                        persistConfig(sanitized, epoch)
                    } finally {
                        if (epoch == sessionEpoch.get()) {
                            configPendingSave = false
                        }
                    }
                if (saveRes.exceptionOrNull() is CancellationException || epoch != sessionEpoch.get()) {
                    return@launch
                }
                if (saveRes.isFailure) {
                    state = state.copy(authenticating = false, error = STORAGE_SAVE_ERROR)
                    return@launch
                }
                if (saveRes.getOrNull() != true) return@launch
                state = state.copy(config = sanitized, error = null)
                probeSavedCredential(sanitized, epoch)
            }
        return null
    }

    suspend fun refresh(): Boolean {
        val gw = gateway ?: return false
        if (state.account == null && state.config?.developmentUserId.isNullOrBlank()) return false
        val epoch = sessionEpoch.get()
        val generation = pollGeneration.incrementAndGet()
        if (state.snapshot === Snapshot.Empty) {
            withContext(Dispatchers.Main.immediate) {
                if (epoch == sessionEpoch.get() && state.snapshot === Snapshot.Empty) {
                    state = state.copy(loading = true)
                }
            }
        }
        val result = withContext(Dispatchers.IO) { runCatching { gw.fetchSnapshot() } }
        return withContext(Dispatchers.Main.immediate) {
            if (generation != pollGeneration.get() || epoch != sessionEpoch.get()) {
                return@withContext false
            }
            val ok =
                result.fold(
                    onSuccess = { snapshot ->
                        val previousHellos =
                            state.snapshot.hosts.associate { it.runnerId to it.hello }
                        val mergedHosts =
                            snapshot.hosts.map { host ->
                                host.copy(
                                    hello = mergeHostHello(host.hello, previousHellos[host.runnerId]),
                                )
                            }
                        state =
                            state.copy(
                                snapshot = snapshot.copy(hosts = mergedHosts),
                                loading = false,
                                error = null,
                                lastUpdatedAt = Instant.now(),
                            )
                        true
                    },
                    onFailure = { failure ->
                        if ((failure as? GatewayException)?.status == 401) {
                            logout(errorMessage = describeGatewayError(failure))
                            return@withContext false
                        }
                        val stale = state.snapshot
                        state =
                            state.copy(
                                loading = false,
                                snapshot =
                                    stale.copy(
                                        hosts = stale.hosts.map { it.copy(online = false) },
                                        sessions = stale.sessions.map { it.copy(online = false) },
                                    ),
                                error = describeGatewayError(failure),
                            )
                        false
                    },
                )
            if (ok) {
                hydrateFleetMetadata()
            }
            ok
        }
    }

    private fun hydrateFleetMetadata() {
        val now = System.currentTimeMillis()
        for (host in state.snapshot.hosts) {
            if (host.revoked) continue
            val last = helloFetchedAtMs[host.runnerId] ?: 0L
            val missingCatalogAndCandidates =
                (host.hello?.models?.isEmpty != false) && host.hello?.candidates.isNullOrEmpty()
            if (missingCatalogAndCandidates || now - last >= HELLO_TTL_MS) {
                scope.launch { loadRunnerHello(host.runnerId, force = false) }
            }
        }
        if (state.plugins == null || now - pluginsFetchedAtMs >= PLUGINS_TTL_MS) {
            scope.launch { loadPlugins(force = false) }
        }
    }

    fun dispatch(
        runnerId: String,
        hubSessionId: String,
        hostGeneration: String?,
        action: JSONObject,
        onResult: (Result<CommandReceipt>) -> Unit,
    ) {
        val gw = gateway
        if (gw == null) {
            onResult(Result.failure(GatewayException(0, "not_configured")))
            return
        }
        val epoch = sessionEpoch.get()
        scope.launch {
            val (hash, clientCmdId) =
                withContext(Dispatchers.Default) {
                    val payload = action.toString()
                    val actionName = action.optNullableString("action").orEmpty()
                    val scopeKey = "$runnerId/$hubSessionId/${hostGeneration.orEmpty()}"
                    val computedHash = ledger.computeHash(scopeKey, actionName, payload)
                    computedHash to ledger.keyForHash(computedHash)
                }
            val result =
                withContext(Dispatchers.IO) {
                    runCatching {
                        gw.submitCommand(
                            runnerId = runnerId,
                            hubSessionId = hubSessionId,
                            hostGeneration = hostGeneration,
                            clientCmdId = clientCmdId,
                            command = action,
                        )
                    }
                }
            if (result.exceptionOrNull() is CancellationException) return@launch
            result
                .onSuccess { ledger.forgetHash(hash) }
                .onFailure {
                    val gwErr = it as? GatewayException
                    ledger.forgetHashIfPermanent(hash, gwErr?.status ?: 0, gwErr?.code.orEmpty())
                }
            if (epoch != sessionEpoch.get()) return@launch
            onResult(result)
        }
    }

    fun reportError(message: String) {
        state = state.copy(error = message)
    }

    fun clearError() {
        if (state.error != null) state = state.copy(error = null)
    }

    fun setAppearance(theme: AppearanceTheme) {
        if (state.appearance == theme) return
        appearanceStore?.save(theme)
        state = state.copy(appearance = theme)
    }

    override fun close() {
        authJob?.cancel()
        authJob = null
        sessionEpoch.incrementAndGet()
        pollGeneration.incrementAndGet()
        scope.cancel()
    }

    private companion object {
        const val HELLO_TTL_MS = 30_000L
        const val PLUGINS_TTL_MS = 60_000L
        const val STORAGE_SAVE_ERROR = "本机安全存储写入失败，请重启应用后重试。"
    }
}
