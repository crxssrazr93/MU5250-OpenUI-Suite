package com.openu60.feature.router.firewall

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.openu60.core.model.FirewallConfig
import com.openu60.core.model.FirewallParser
import com.openu60.core.model.PortForwardRule
import com.openu60.core.network.AgentClient
import com.openu60.core.network.AgentError
import com.openu60.core.network.AuthManager
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

data class FirewallSettingsState(
    val config: FirewallConfig = FirewallConfig.empty,
    val portForwardRules: List<PortForwardRule> = emptyList(),
    val isLoading: Boolean = false,
    val message: String? = null,
    val messageIsError: Boolean = false,
    val showPortForwardForm: Boolean = false,
)

@HiltViewModel
class FirewallSettingsViewModel @Inject constructor(
    private val agentClient: AgentClient,
    private val authManager: AuthManager,
) : ViewModel() {

    private val _state = MutableStateFlow(FirewallSettingsState())
    val state: StateFlow<FirewallSettingsState> = _state.asStateFlow()

    fun refresh() {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                val configData = agentClient.getJSON("/api/firewall/config")
                val config = FirewallParser.parseConfig(configData)

                val pfData = agentClient.getJSON("/api/firewall/port-forward")
                val rules = FirewallParser.parsePortForwardRules(pfData)

                _state.value = _state.value.copy(config = config, portForwardRules = rules, isLoading = false)
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) refresh() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    // Booleans, not "1"/"0" strings: the agent reads these with `as_bool`, and
    // a string is not a bool, so every write used to come back
    // "nothing to change — no known setting was given".
    //
    // Only these three have a setter. NAT, DMZ and the filter policy are
    // reported by the agent but not writable by it, and the vendor returns an
    // empty firewall `level`, so the level selector is gone rather than left
    // sending a value nothing reads.
    fun toggleFirewall(enabled: Boolean) = updateConfig(mapOf("firewall_enabled" to enabled))
    fun togglePortForward(enabled: Boolean) = updateConfig(mapOf("port_forward_enabled" to enabled))
    fun togglePortMapping(enabled: Boolean) = updateConfig(mapOf("port_mapping_enabled" to enabled))

    fun addPortForwardRule(name: String, protocol: String, wanPort: String, lanIP: String, lanPort: String, enabled: Boolean) {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                agentClient.postConfirmedJSON("/api/firewall/port-forward", mapOf(
                    "action" to "add",
                    "comment" to name,
                    "proto" to protocol.uppercase(),
                    "src_dport" to wanPort,
                    "dest_ip" to lanIP,
                    "dest_port" to lanPort,
                    "enabled" to enabled,
                ))
                _state.value = _state.value.copy(
                    showPortForwardForm = false,
                    message = "Rule added",
                    messageIsError = false,
                )
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) addPortForwardRule(name, protocol, wanPort, lanIP, lanPort, enabled)
                else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun deletePortForwardRule(id: String) {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                // `section_id` is a list because the vendor's setter takes one,
                // and delete goes through the same POST route as add.
                agentClient.postConfirmedJSON(
                    "/api/firewall/port-forward",
                    mapOf("action" to "delete", "section_id" to listOf(id)),
                )
                _state.value = _state.value.copy(message = "Rule deleted", messageIsError = false)
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) deletePortForwardRule(id) else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun showAddForm() {
        _state.value = _state.value.copy(showPortForwardForm = true)
    }

    fun hideAddForm() {
        _state.value = _state.value.copy(showPortForwardForm = false)
    }

    private fun updateConfig(params: Map<String, Any?>) {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                agentClient.postConfirmedJSON("/api/firewall/config", params)
                _state.value = _state.value.copy(message = "Settings updated", messageIsError = false)
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) updateConfig(params) else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    private fun setError(msg: String?) {
        _state.value = _state.value.copy(isLoading = false, message = msg ?: "Unknown error", messageIsError = true)
    }
}
