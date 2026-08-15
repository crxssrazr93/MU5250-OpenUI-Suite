package com.openu60.feature.bandlock

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
import java.math.BigInteger
import javax.inject.Inject

data class BandLockState(
    val selectedNRBands: Set<Int> = emptySet(),
    val selectedLTEBands: Set<Int> = emptySet(),
    /** What the modem is locked to right now, as opposed to what is ticked. */
    val lockedNRBands: Set<Int> = emptySet(),
    val lockedLTEBands: Set<Int> = emptySet(),
    val isLoading: Boolean = false,
    val error: String? = null,
    val successMessage: String? = null,
)

@HiltViewModel
class BandLockViewModel @Inject constructor(
    private val agentClient: AgentClient,
    private val authManager: AuthManager,
) : ViewModel() {

    private val _state = MutableStateFlow(BandLockState())
    val state: StateFlow<BandLockState> = _state.asStateFlow()

    /**
     * Read back what the modem is actually locked to.
     *
     * There is no band-lock getter; the current lock is reported in the same
     * raw netinfo object the signal screen reads. Without this the screen
     * always opened blank, which reads as "nothing is locked" whether or not
     * anything is.
     */
    fun refresh() {
        viewModelScope.launch {
            _state.value = _state.value.copy(error = null)
            try {
                val data = agentClient.getJSON("/api/network/signal")
                val lte = lteBandsFromMask(data["lte_band_lock"] as? String)
                val nr = nrBandsFromList(
                    (data["nr5g_sa_band_lock"] as? String).orEmpty()
                        .ifEmpty { (data["nr5g_nsa_band_lock"] as? String).orEmpty() },
                )
                _state.value = _state.value.copy(
                    lockedNRBands = nr,
                    lockedLTEBands = lte,
                    // Only seed the ticks on the first read. Re-seeding on every
                    // refresh would undo a selection the user is still making.
                    selectedNRBands = if (_state.value.selectedNRBands.isEmpty()) nr else _state.value.selectedNRBands,
                    selectedLTEBands = if (_state.value.selectedLTEBands.isEmpty()) lte else _state.value.selectedLTEBands,
                )
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) refresh()
                else _state.value = _state.value.copy(error = e.message)
            } catch (e: Exception) {
                _state.value = _state.value.copy(error = e.message)
            }
        }
    }

    fun toggleNRBand(band: Int) {
        val current = _state.value.selectedNRBands.toMutableSet()
        if (band in current) current.remove(band) else current.add(band)
        _state.value = _state.value.copy(selectedNRBands = current, successMessage = null)
    }

    fun toggleLTEBand(band: Int) {
        val current = _state.value.selectedLTEBands.toMutableSet()
        if (band in current) current.remove(band) else current.add(band)
        _state.value = _state.value.copy(selectedLTEBands = current, successMessage = null)
    }

    fun applyNRLock() {
        val bands = _state.value.selectedNRBands.sorted()
        if (bands.isEmpty()) return
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, error = null, successMessage = null)
            try {
                // One call, not one per NR type. The agent forces SA: NSA band
                // locking does not take on this firmware, so sending it would
                // report success for a write that changes nothing.
                agentClient.postConfirmedJSON("/api/modem/bands/nr/lock", mapOf("bands" to bands))
                _state.value = _state.value.copy(
                    isLoading = false,
                    successMessage = "NR bands locked to: ${bands.joinToString(", ") { "n$it" }}",
                )
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) applyNRLock()
                else _state.value = _state.value.copy(isLoading = false, error = e.message)
            } catch (e: Exception) {
                _state.value = _state.value.copy(isLoading = false, error = e.message)
            }
        }
    }

    fun applyLTELock() {
        val bands = _state.value.selectedLTEBands.sorted()
        if (bands.isEmpty()) return
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, error = null, successMessage = null)
            try {
                // The vendor wants a decimal bitmask, not a band list. The agent
                // builds it — see compat::lte_band_mask — because this app sent
                // "1,3,8" as if it were a mask and locked to bands 1, 2, 4, 8,
                // 16 and 32 instead.
                agentClient.postConfirmedJSON("/api/modem/bands/lte/lock", mapOf("bands" to bands))
                _state.value = _state.value.copy(
                    isLoading = false,
                    successMessage = "LTE bands locked to: ${bands.joinToString(", ") { "B$it" }}",
                )
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) applyLTELock()
                else _state.value = _state.value.copy(isLoading = false, error = e.message)
            } catch (e: Exception) {
                _state.value = _state.value.copy(isLoading = false, error = e.message)
            }
        }
    }

    fun unlockAll() {
        viewModelScope.launch {
            _state.value = _state.value.copy(isLoading = true, error = null, successMessage = null)
            try {
                // An empty body is how this route asks for a reset; there is no
                // DELETE verb on it.
                agentClient.postConfirmedJSON("/api/modem/bands/lock")
                _state.value = _state.value.copy(
                    isLoading = false,
                    selectedNRBands = emptySet(),
                    selectedLTEBands = emptySet(),
                    lockedNRBands = emptySet(),
                    lockedLTEBands = emptySet(),
                    successMessage = "All bands unlocked",
                )
                refresh()
            } catch (e: AgentError.Unauthorized) {
                if (authManager.reauthenticate()) unlockAll()
                else _state.value = _state.value.copy(isLoading = false, error = e.message)
            } catch (e: Exception) {
                _state.value = _state.value.copy(isLoading = false, error = e.message)
            }
        }
    }

    companion object {
        /**
         * Band N sits at bit N-1 of the mask. "0" means no lock.
         *
         * The mask arrives hex-prefixed — this unit reports
         * `lte_band_lock: "0x87e29a0e00df"` — while the setter takes decimal.
         * Both spellings are accepted here: `BigInteger("0x…")` throws, so
         * reading only decimal silently reported no lock at all on hardware
         * that had one.
         */
        fun lteBandsFromMask(raw: String?): Set<Int> {
            val text = raw?.trim().orEmpty()
            if (text.isEmpty() || text == "0" || text == "0x0") return emptySet()
            val mask = try {
                if (text.startsWith("0x", ignoreCase = true)) {
                    BigInteger(text.substring(2), 16)
                } else {
                    BigInteger(text)
                }
            } catch (_: NumberFormatException) {
                return emptySet()
            }
            // B66 alone needs 66 bits, so this cannot be a Long.
            return (1..128).filter { mask.testBit(it - 1) }.toSet()
        }

        fun nrBandsFromList(raw: String?): Set<Int> =
            raw.orEmpty()
                .split(',')
                .mapNotNull { it.trim().trimStart('n', 'N').toIntOrNull() }
                .filter { it > 0 }
                .toSet()
    }
}
