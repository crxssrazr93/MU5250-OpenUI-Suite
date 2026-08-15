package com.openu60.feature.router.operator

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.openu60.core.model.OperatorParser
import com.openu60.core.model.OperatorScan
import com.openu60.core.model.ScannedOperator
import com.openu60.core.network.AgentClient
import com.openu60.core.network.AgentError
import com.openu60.core.network.AuthManager
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

data class OperatorUiState(
    val scan: OperatorScan = OperatorScan.empty,
    val loaded: Boolean = false,
    val busy: Boolean = false,
    val message: String? = null,
    val messageIsError: Boolean = false,
)

/**
 * Manual operator selection, matching the dashboard's Operator tab.
 *
 * A scan sweeps every band for reachable networks (about 40 seconds, with
 * mobile data down for the duration), then one can be registered on by hand or
 * left to automatic. The scan is polled only while it is actually running.
 */
@HiltViewModel
class OperatorViewModel @Inject constructor(
    private val agentClient: AgentClient,
    private val authManager: AuthManager,
) : ViewModel() {

    private val _state = MutableStateFlow(OperatorUiState())
    val state: StateFlow<OperatorUiState> = _state.asStateFlow()

    private var pollJob: Job? = null

    fun refresh() {
        viewModelScope.launch { readOnce() }
    }

    private suspend fun readOnce() {
        try {
            val scan = OperatorParser.parseScan(agentClient.getSlowJSON("/api/operator/scan"))
            _state.value = _state.value.copy(scan = scan, loaded = true)
            if (scan.scanning) startPolling() else stopPolling()
        } catch (e: AgentError.Unauthorized) {
            if (!authManager.reauthenticate()) setError(e.message)
        } catch (_: Exception) {
            // A modem that will not answer is better shown as quiet than as an
            // error that wipes the previous results.
            _state.value = _state.value.copy(loaded = true)
        }
    }

    private fun startPolling() {
        if (pollJob?.isActive == true) return
        pollJob = viewModelScope.launch {
            while (_state.value.scan.scanning) {
                delay(3000)
                readOnce()
            }
        }
    }

    private fun stopPolling() {
        pollJob?.cancel()
        pollJob = null
    }

    fun startScan() {
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, message = null)
            try {
                agentClient.postConfirmedJSON("/api/operator/scan/start", slow = true)
                _state.value = _state.value.copy(busy = false, message = "Scanning…", messageIsError = false)
                readOnce()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) startScan() else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun select(operator: ScannedOperator) {
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, message = null)
            try {
                agentClient.postConfirmedJSON(
                    "/api/operator/select",
                    mapOf("select" to operator.select),
                    slow = true,
                )
                _state.value = _state.value.copy(busy = false, message = "Registering on ${operator.name}…", messageIsError = false)
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) select(operator) else setError(e.message)
            } catch (e: Exception) {
                setError(e.message)
            }
        }
    }

    fun automatic() {
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, message = null)
            try {
                agentClient.postConfirmedJSON("/api/operator/select", mapOf("auto" to true), slow = true)
                _state.value = _state.value.copy(busy = false, message = "Back to automatic selection", messageIsError = false)
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) automatic() else setError(e.message)
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

    override fun onCleared() {
        stopPolling()
        super.onCleared()
    }
}
