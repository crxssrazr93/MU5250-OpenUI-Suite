package com.openu60.feature.router.telemetry

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.openu60.core.model.DomainFilterConfig
import com.openu60.core.model.TelemetryParser
import com.openu60.core.network.AgentClient
import com.openu60.core.network.AgentError
import com.openu60.core.network.AuthManager
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

data class TelemetryBlockerState(
    val config: DomainFilterConfig = DomainFilterConfig.empty,
    val isLoading: Boolean = false,
    val message: String? = null,
    val messageIsError: Boolean = false,
    val newDomain: String = "",
)

@HiltViewModel
class TelemetryBlockerViewModel @Inject constructor(
    private val agentClient: AgentClient,
    private val authManager: AuthManager,
) : ViewModel() {

    private val _state = MutableStateFlow(TelemetryBlockerState())
    val state: StateFlow<TelemetryBlockerState> = _state.asStateFlow()

    fun refresh() {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                val data = agentClient.getJSON("/api/firewall/domain-filter")
                val config = TelemetryParser.parseDomainFilter(data)
                _state.value = _state.value.copy(config = config, isLoading = false)
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) refresh() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    // There is no filter-wide on/off switch. The firmware filters per rule, and
    // the toggle that used to sit here PUT to a route that only serves GET. A
    // switch that reports a state nothing stores is worse than no switch — the
    // same call made for Smart Tower Connect and signal detect.

    fun updateNewDomain(value: String) {
        _state.value = _state.value.copy(newDomain = value)
    }

    fun addRule(domain: String) {
        if (domain.isBlank()) return
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                addOne(domain)
                _state.value = _state.value.copy(newDomain = "", message = "Rule added", messageIsError = false)
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) addRule(domain) else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun removeRule(id: String) {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                // Add, edit and delete all go through one POST, told apart by
                // `action`; `section_id` is a list because the vendor setter
                // takes one.
                agentClient.postConfirmedJSON(
                    "/api/firewall/domain-filter/rule",
                    mapOf("action" to "delete", "section_id" to listOf(id)),
                )
                _state.value = _state.value.copy(message = "Rule removed", messageIsError = false)
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) removeRule(id) else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun blockAllTelemetry() {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, message = null)
            try {
                val existing = _state.value.config.rules.map { it.domain }.toSet()
                var added = 0
                for (domain in TelemetryParser.knownTelemetryDomains) {
                    if (domain !in existing) {
                        addOne(domain)
                        added++
                    }
                }
                _state.value = _state.value.copy(
                    message = if (added > 0) "Blocked $added telemetry domains" else "All telemetry domains already blocked",
                    messageIsError = false,
                )
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) blockAllTelemetry() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    /**
     * One blocked domain.
     *
     * `fqdn`, not `domain` — the agent validates the name under that key and
     * passes it to the vendor's `router_set_domain_filter`. `target` defaults
     * to DROP at the agent, but it is sent explicitly here because a blocklist
     * that quietly defaulted to ACCEPT would read as blocking while allowing.
     */
    private suspend fun addOne(domain: String) {
        agentClient.postConfirmedJSON(
            "/api/firewall/domain-filter/rule",
            mapOf(
                "action" to "add",
                "fqdn" to domain,
                "enable" to true,
                "target" to "DROP",
            ),
        )
    }

    private fun setError(msg: String?) {
        _state.value = _state.value.copy(isLoading = false, message = msg ?: "Unknown error", messageIsError = true)
    }
}
