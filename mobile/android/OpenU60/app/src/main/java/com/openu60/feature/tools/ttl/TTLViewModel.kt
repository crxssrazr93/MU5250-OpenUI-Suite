package com.openu60.feature.tools.ttl

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.openu60.core.network.AgentClient
import com.openu60.core.network.AgentError
import com.openu60.core.network.AuthManager
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

data class TTLState(
    val loaded: Boolean = false,
    val active: Boolean = false,
    val ipv6Active: Boolean = false,
    val ttlValue: Int = 0,
    val busy: Boolean = false,
    val message: String? = null,
    val messageIsError: Boolean = false,
)

/**
 * TTL / hop-limit override, matching the dashboard's TTL tab.
 *
 * Rewrites the TTL on LAN ingress traffic so a carrier cannot tell tethered
 * traffic apart by its decremented hop count. The agent applies it to both
 * IPv4 and IPv6 and persists it across reboots.
 */
@HiltViewModel
class TTLViewModel @Inject constructor(
    private val agentClient: AgentClient,
    private val authManager: AuthManager,
) : ViewModel() {

    private val _state = MutableStateFlow(TTLState())
    val state: StateFlow<TTLState> = _state.asStateFlow()

    fun refresh() {
        viewModelScope.launch {
            try {
                val data = agentClient.getJSON("/api/ttl/status")
                _state.value = _state.value.copy(
                    loaded = true,
                    active = data["active"] == true,
                    ipv6Active = data["ipv6_active"] == true,
                    ttlValue = (data["ttl_value"] as? Number)?.toInt() ?: 0,
                )
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) refresh() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun setTtl(value: Int) {
        if (value < 1 || value > 255) {
            _state.value = _state.value.copy(message = "TTL must be 1–255", messageIsError = true)
            return
        }
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, message = null)
            try {
                agentClient.putJSON("/api/ttl/set", mapOf("ttl" to value))
                _state.value = _state.value.copy(busy = false, message = "TTL set to $value (IPv4 + IPv6)", messageIsError = false)
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) setTtl(value) else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun clear() {
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, message = null)
            try {
                agentClient.deleteJSON("/api/ttl/clear")
                _state.value = _state.value.copy(busy = false, message = "TTL override disabled", messageIsError = false)
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) clear() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun clearMessage() {
        _state.value = _state.value.copy(message = null)
    }

    private fun setError(msg: String?) {
        _state.value = _state.value.copy(busy = false, message = msg ?: "Unknown error", messageIsError = true)
    }
}
