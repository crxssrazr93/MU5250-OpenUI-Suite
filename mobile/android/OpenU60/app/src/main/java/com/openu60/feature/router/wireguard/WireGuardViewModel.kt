package com.openu60.feature.router.wireguard

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.openu60.core.model.WireguardParser
import com.openu60.core.model.WireguardProfile
import com.openu60.core.model.WireguardState
import com.openu60.core.network.AgentClient
import com.openu60.core.network.AgentError
import com.openu60.core.network.AuthManager
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

data class WireGuardUiState(
    val tunnel: WireguardState = WireguardState.empty,
    val profiles: List<WireguardProfile> = emptyList(),
    /** The form, seeded from the vendor config and edited by the user. */
    val draft: Map<String, String> = emptyMap(),
    /** Shown once after a keygen; the private half never leaves the router. */
    val generatedPublicKey: String? = null,
    val isLoading: Boolean = false,
    val busy: Boolean = false,
    val message: String? = null,
    val messageIsError: Boolean = false,
) {
    val missingToConnect: List<String>
        get() = WireguardParser.requiredToConnect.filter { draft[it].isNullOrBlank() }
    val canConnect: Boolean get() = tunnel.configured && missingToConnect.isEmpty()
}

@HiltViewModel
class WireGuardViewModel @Inject constructor(
    private val agentClient: AgentClient,
    private val authManager: AuthManager,
) : ViewModel() {

    private val _state = MutableStateFlow(WireGuardUiState())
    val state: StateFlow<WireGuardUiState> = _state.asStateFlow()

    fun refresh() {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                val tunnel = WireguardParser.parseState(agentClient.getJSON("/api/tunnel/wireguard"))
                val profiles = WireguardParser.parseProfiles(
                    agentClient.getJSON("/api/tunnel/wireguard/profiles"),
                )
                _state.value = _state.value.copy(
                    tunnel = tunnel,
                    profiles = profiles,
                    // Only reseed the form from the router when the user is not
                    // mid-edit, so a background refresh does not wipe typing.
                    draft = if (_state.value.draft.isEmpty()) tunnel.settings else _state.value.draft,
                    isLoading = false,
                )
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) refresh() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun updateField(key: String, value: String) {
        _state.value = _state.value.copy(draft = _state.value.draft + (key to value))
    }

    fun save() {
        viewModelScope.launch {
            // Only what changed. Sending the whole form back would rewrite
            // untouched fields, and the masked ones as asterisks.
            val changed = _state.value.draft.filter { (k, v) -> v != (_state.value.tunnel.settings[k] ?: "") }
            if (changed.isEmpty()) {
                _state.value = _state.value.copy(message = "Nothing changed", messageIsError = false)
                return@launch
            }
            run("Saving", "Settings saved") { agentClient.postSlowJSON("/api/tunnel/wireguard", changed) }
        }
    }

    fun keygen() {
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, message = null)
            try {
                val data = agentClient.postConfirmedJSON("/api/tunnel/wireguard/keygen")
                _state.value = _state.value.copy(
                    busy = false,
                    generatedPublicKey = data["public_key"]?.toString(),
                    message = "Key pair generated",
                    messageIsError = false,
                )
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) keygen() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun setConnected(connect: Boolean) {
        val path = if (connect) "/api/tunnel/wireguard/connect" else "/api/tunnel/wireguard/disconnect"
        run(if (connect) "Starting tunnel" else "Stopping tunnel",
            if (connect) "Tunnel starting" else "Tunnel stopped") {
            agentClient.postConfirmedJSON(path, emptyMap(), slow = true)
        }
    }

    fun importProfile(name: String, conf: String) {
        if (conf.isBlank()) {
            _state.value = _state.value.copy(message = "Paste a config or choose a .conf file", messageIsError = true)
            return
        }
        run("Importing", "Profile saved") {
            agentClient.postSlowJSON(
                "/api/tunnel/wireguard/profiles",
                mapOf("name" to name.ifBlank { "Untitled tunnel" }, "conf" to conf),
            )
        }
    }

    fun activateProfile(profile: WireguardProfile) {
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, message = null)
            try {
                val data = agentClient.postConfirmedJSON(
                    "/api/tunnel/wireguard/profiles/activate",
                    mapOf("id" to profile.id),
                    slow = true,
                )
                val endpoint = data["endpoint"]?.toString()
                _state.value = _state.value.copy(
                    busy = false,
                    // The activated config replaces the vendor settings, so drop
                    // the draft and let refresh reseed it.
                    draft = emptyMap(),
                    message = if (!endpoint.isNullOrBlank()) "${profile.name} is ready — endpoint $endpoint"
                    else "${profile.name} is ready",
                    messageIsError = false,
                )
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) activateProfile(profile) else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun deleteProfile(profile: WireguardProfile) {
        run("Deleting", "Profile deleted") {
            agentClient.postConfirmedJSON(
                "/api/tunnel/wireguard/profiles/delete",
                mapOf("id" to profile.id),
                slow = true,
            )
        }
    }

    fun clearMessage() {
        _state.value = _state.value.copy(message = null)
    }

    private fun run(busyLabel: String, successLabel: String, work: suspend () -> Map<String, Any?>) {
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, message = null)
            try {
                work()
                _state.value = _state.value.copy(busy = false, message = successLabel, messageIsError = false)
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) run(busyLabel, successLabel, work) else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    private fun setError(msg: String?) {
        _state.value = _state.value.copy(
            isLoading = false, busy = false, message = msg ?: "Unknown error", messageIsError = true,
        )
    }
}
