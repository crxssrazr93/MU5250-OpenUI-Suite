package com.openu60.feature.router.esim

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.openu60.core.model.EuiccChipInfo
import com.openu60.core.model.EuiccDownloadRequest
import com.openu60.core.model.EuiccNotification
import com.openu60.core.model.EuiccOperation
import com.openu60.core.model.EuiccParser
import com.openu60.core.model.EuiccProfile
import com.openu60.core.model.EuiccStatus
import com.openu60.core.network.AgentClient
import com.openu60.core.network.AgentError
import com.openu60.core.network.AuthManager
import com.openu60.core.network.CapabilityProvider
import com.openu60.core.network.EuiccRelay
import com.openu60.core.network.Feature
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

data class ESIMState(
    val status: EuiccStatus = EuiccStatus.empty,
    val eid: String? = null,
    val profiles: List<EuiccProfile> = emptyList(),
    /**
     * Only the ones that could not be delivered. The agent sends each
     * notification inside the operation that created it, so anything here has
     * already failed at least once.
     */
    val undeliverableNotifications: List<EuiccNotification> = emptyList(),
    val chip: EuiccChipInfo? = null,
    val revealed: Boolean = false,
    val canWrite: Boolean = true,
    val canRelay: Boolean = true,
    val isLoading: Boolean = false,
    val busyWith: String? = null,
    val progress: List<String> = emptyList(),
    /** Set after a switch the modem could not be told about. */
    val rebootNotice: String? = null,
    val message: String? = null,
    val messageIsError: Boolean = false,
)

@HiltViewModel
class ESIMViewModel @Inject constructor(
    private val agentClient: AgentClient,
    private val authManager: AuthManager,
    private val capabilities: CapabilityProvider,
    val relay: EuiccRelay,
) : ViewModel() {

    private val _state = MutableStateFlow(ESIMState())
    val state: StateFlow<ESIMState> = _state.asStateFlow()

    private var relayJob: Job? = null

    /** Whether a LAN client is carrying this router's operator traffic. */
    private var useRelay = false

    fun setUseRelay(enabled: Boolean) {
        useRelay = enabled
        if (enabled) startRelay() else stopRelay()
    }

    fun refresh() {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                val canWrite = capabilities.has(Feature.EUICC_WRITE)
                val canRelay = capabilities.has(Feature.EUICC_HTTP_RELAY)
                val status = EuiccParser.parseStatus(agentClient.getSlowJSON("/api/euicc/status"))

                if (!status.euiccAvailable) {
                    _state.value = _state.value.copy(
                        status = status, profiles = emptyList(), eid = null,
                        canWrite = canWrite, canRelay = canRelay, isLoading = false,
                    )
                    return@launch
                }

                // Card access is serialized behind one mutex on the agent, so
                // these are sequential by necessity, not for convenience.
                val full = if (_state.value.revealed) "?full=true" else ""
                val eid = EuiccParser.parseEid(agentClient.getSlowJSON("/api/euicc/eid$full"))
                val profiles = EuiccParser.parseProfiles(
                    agentClient.getSlowJSON("/api/euicc/profiles$full")
                )
                _state.value = _state.value.copy(
                    status = status, eid = eid, profiles = profiles,
                    canWrite = canWrite, canRelay = canRelay, isLoading = false,
                )
                if (canWrite) loadNotifications()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) refresh() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    /** Identifiers are masked by default; revealing is an explicit action. */
    fun toggleReveal() {
        _state.value = _state.value.copy(revealed = !_state.value.revealed)
        refresh()
    }

    fun loadChipInfo() {
        viewModelScope.launch {
            runCatching { EuiccParser.parseChipInfo(agentClient.getSlowJSON("/api/euicc/chip")) }
                .onSuccess { _state.value = _state.value.copy(chip = it) }
                .onFailure { setError(it.message) }
        }
    }

    fun download(request: EuiccDownloadRequest) {
        viewModelScope.launch {
            if (request.useRelay || useRelay) startRelay()
            run("Downloading profile", "Profile installed") {
                agentClient.postConfirmedJSON("/api/euicc/download", request.toBody(), slow = true)
            }
            if (request.useRelay && !useRelay) stopRelay()
        }
    }

    fun enable(profile: EuiccProfile) {
        viewModelScope.launch {
            val iccid = resolveIccid(profile) ?: return@launch
            run("Enabling", "Profile enabled") {
                agentClient.postConfirmedJSON(
                    "/api/euicc/enable",
                    // Off by design: this modem rejects EnableProfile with the
                    // refresh flag, so the agent reports reboot_required instead.
                    mapOf("iccid" to iccid, "refresh" to false, "relay" to useRelay),
                    slow = true,
                )
            }
        }
    }

    fun disable(profile: EuiccProfile, force: Boolean) {
        viewModelScope.launch {
            val iccid = resolveIccid(profile) ?: return@launch
            run("Disabling", "Profile disabled") {
                agentClient.postConfirmedJSON(
                    "/api/euicc/disable",
                    mapOf("iccid" to iccid, "force" to force, "relay" to useRelay),
                    slow = true,
                )
            }
        }
    }

    fun delete(profile: EuiccProfile) {
        viewModelScope.launch {
            val iccid = resolveIccid(profile) ?: return@launch
            run("Deleting", "Profile deleted") {
                agentClient.postConfirmedJSON(
                    "/api/euicc/delete",
                    mapOf("iccid" to iccid, "relay" to useRelay),
                    slow = true,
                )
            }
        }
    }

    fun rename(profile: EuiccProfile, nickname: String) {
        viewModelScope.launch {
            val iccid = resolveIccid(profile) ?: return@launch
            run("Renaming", "Renamed") {
                agentClient.postSlowJSON(
                    "/api/euicc/nickname",
                    mapOf("iccid" to iccid, "nickname" to nickname.trim()),
                )
            }
        }
    }

    /**
     * Read what the card is still waiting to report.
     *
     * Nothing is sent from here any more. The agent delivers each notification
     * inside the operation that created it, while it still holds the card and
     * the relay — the only point at which delivery is reliable. Enabling ends
     * in a reboot that would take this screen with it, and a download's relay
     * is gone by the time the app could ask for a separate send.
     *
     * So anything listed here has already failed at least once, which is worth
     * showing and worth an explicit retry rather than a silent one.
     */
    private suspend fun loadNotifications() {
        val pending = runCatching {
            EuiccParser.parseNotifications(agentClient.getSlowJSON("/api/euicc/notifications"))
        }.getOrDefault(emptyList())
        _state.value = _state.value.copy(undeliverableNotifications = pending)
    }

    /** Try the stuck ones again — e.g. after connecting a relay. */
    fun retryNotifications() {
        viewModelScope.launch {
            val sequences = _state.value.undeliverableNotifications.map { it.seqNumber }
            if (sequences.isEmpty()) return@launch
            runCatching {
                agentClient.postSlowJSON(
                    "/api/euicc/notifications/process",
                    mapOf("sequences" to sequences, "relay" to useRelay),
                )
            }.onFailure { setError(it.message) }
            loadNotifications()
        }
    }

    fun discardNotifications(sequences: List<Int>) {
        viewModelScope.launch {
            run("Discarding", "Notifications discarded") {
                agentClient.postConfirmedJSON(
                    "/api/euicc/notifications/remove",
                    mapOf("sequences" to sequences),
                )
            }
        }
    }

    fun dismissRebootNotice() {
        _state.value = _state.value.copy(rebootNotice = null)
    }

    fun clearMessage() {
        _state.value = _state.value.copy(message = null, progress = emptyList())
    }

    fun reboot() {
        viewModelScope.launch {
            runCatching { agentClient.postConfirmedJSON("/api/device/reboot") }
                .onSuccess {
                    _state.value = _state.value.copy(
                        rebootNotice = null, message = "Rebooting…", messageIsError = false,
                    )
                }
                .onFailure { setError(it.message) }
        }
    }

    // ── Internals ────────────────────────────────────────────────────────────

    /**
     * An operation is addressed by ICCID, and the list may only be holding it
     * masked. Masking is about what is on screen, so it must not decide what
     * the user is allowed to do: the ISD-P AID is never masked and is unique
     * per profile, so it identifies the same row in the unmasked list.
     */
    private suspend fun resolveIccid(profile: EuiccProfile): String? {
        profile.iccid?.takeIf { !profile.iccidIsMasked }?.let { return it }
        val full = runCatching {
            EuiccParser.parseProfiles(agentClient.getSlowJSON("/api/euicc/profiles?full=true"))
        }.getOrNull()
        val match = full?.firstOrNull { it.isdpAid != null && it.isdpAid == profile.isdpAid }
        if (match?.iccid == null) {
            setError("Could not identify that profile on the card.")
            return null
        }
        return match.iccid
    }

    private suspend fun run(
        busyLabel: String,
        successLabel: String,
        work: suspend () -> Map<String, Any?>,
    ) {
        _state.value = _state.value.copy(busyWith = busyLabel, message = null, progress = emptyList())
        try {
            val operation: EuiccOperation = EuiccParser.parseOperation(work())
            _state.value = _state.value.copy(
                busyWith = null,
                progress = operation.progress,
                message = successLabel,
                messageIsError = false,
                rebootNotice = operation.notice.takeIf { operation.rebootRequired },
            )
            refresh()
        } catch (e: AgentError.Unauthorized) {
            if (authManager.reauthenticate()) run(busyLabel, successLabel, work) else setError(e.message)
        } catch (e: Exception) {
            setError(e.message)
        }
    }

    private fun relayIsRunning(): Boolean = relayJob?.isActive == true

    /**
     * Start carrying traffic for the router.
     *
     * Called when the eSIM screen opens, not just around a download: the agent
     * parks a request and waits, so a relay started *after* the request is
     * parked still works, but starting first removes the race and lets the
     * automatic notification flush use it too.
     *
     * Deliberately tied to the screen rather than run forever. Holding a long
     * poll open for the life of the app costs a wakelock-shaped amount of
     * battery for a feature used a handful of times, and Android would require
     * a foreground service with a permanent notification to do it honestly. If
     * you want that, it belongs behind an explicit "keep relay running" switch,
     * not as the default.
     */
    fun startRelay() {
        if (relayIsRunning()) return
        relayJob = viewModelScope.launch { relay.run() }
    }

    fun stopRelay() {
        relayJob?.cancel()
        relayJob = null
    }

    private fun setError(msg: String?) {
        _state.value = _state.value.copy(
            isLoading = false,
            busyWith = null,
            message = msg ?: "Unknown error",
            messageIsError = true,
        )
    }

    override fun onCleared() {
        stopRelay()
        super.onCleared()
    }
}
