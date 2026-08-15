package com.openu60.feature.router

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp

@OptIn(ExperimentalMaterial3Api::class)
@Composable
/**
 * Entries are missing for STK, Guest WiFi and QoS on purpose.
 *
 * STK has no firmware surface on this device at all — the same gap that stops
 * an eSIM profile switch taking effect without a reboot. Guest WiFi and QoS do
 * work, but the stock web UI already provides them, so duplicating them here
 * would mean two controls over one setting.
 *
 * See docs/MOBILE-API-GAP.md. The screens remain in the source; only the way in
 * is gone, so restoring one is a single line.
 */
fun RouterSettingsListScreen(
    onNavigateToMobileNetwork: () -> Unit,
    onNavigateToNetworkMode: () -> Unit,
    onNavigateToOperator: () -> Unit,
    onNavigateToCellLock: () -> Unit,
    onNavigateToSTC: () -> Unit,
    onNavigateToSignalDetect: () -> Unit,
    onNavigateToSIM: () -> Unit,
    onNavigateToESIM: () -> Unit,
    onNavigateToSTK: () -> Unit,
    onNavigateToWiFi: () -> Unit,
    onNavigateToGuestWiFi: () -> Unit,
    onNavigateToAPN: () -> Unit,
    onNavigateToLAN: () -> Unit,
    onNavigateToDNS: () -> Unit,
    onNavigateToWireGuard: () -> Unit,
    onNavigateToFirewall: () -> Unit,
    onNavigateToTelemetryBlocker: () -> Unit,
    onNavigateToVPNPassthrough: () -> Unit,
    onNavigateToQoS: () -> Unit,
    onNavigateToDeviceControl: () -> Unit,
    onNavigateToScheduleReboot: () -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(title = { Text("Router Settings") })
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            // Cellular
            SectionHeader("Cellular")
            SettingsItem(Icons.Default.CellTower, "Mobile Network", onClick = onNavigateToMobileNetwork)
            SettingsItem(Icons.Default.SettingsInputAntenna, "Network Mode", onClick = onNavigateToNetworkMode)
            SettingsItem(Icons.Default.NetworkCell, "Operator Selection", onClick = onNavigateToOperator)
            SettingsItem(Icons.Default.Lock, "Cell Lock", onClick = onNavigateToCellLock)
            // No Smart Tower Connect entry either. The endpoints are served and
            // return the vendor's real parameters, but enable and disable change
            // nothing observable on this unit and there is no state to read
            // back, so the toggle would sit there doing nothing. Measured, not
            // assumed — see the note in the agent's cell.rs.
            //
            // No Signal Detection entry: the vendor's detect methods are a
            // manual, location-tagged site survey, not the band sweep this
            // screen renders, so it could only ever open on an error. The
            // screen is left in the tree; see docs/MOBILE-API-GAP.md.
            SettingsItem(Icons.Default.SimCard, "SIM Card", onClick = onNavigateToSIM)
            SettingsItem(Icons.Default.SimCard, "eSIM", onClick = onNavigateToESIM)

            Spacer(modifier = Modifier.height(8.dp))

            // Connectivity
            SectionHeader("Connectivity")
            SettingsItem(Icons.Default.Wifi, "WiFi", onClick = onNavigateToWiFi)
            SettingsItem(Icons.Default.Language, "APN", onClick = onNavigateToAPN)
            SettingsItem(Icons.Default.Router, "LAN / DHCP", onClick = onNavigateToLAN)
            SettingsItem(Icons.Default.Dns, "DNS", onClick = onNavigateToDNS)
            SettingsItem(Icons.Default.VpnKey, "WireGuard VPN", onClick = onNavigateToWireGuard)

            Spacer(modifier = Modifier.height(8.dp))

            // Security
            SectionHeader("Security")
            SettingsItem(Icons.Default.Shield, "Firewall", onClick = onNavigateToFirewall)
            SettingsItem(Icons.Default.VisibilityOff, "Telemetry Blocker", onClick = onNavigateToTelemetryBlocker)
            // No VPN Passthrough entry: /api/vpn/passthrough is not served, and
            // no vendor surface for the L2TP/PPTP/IPsec flags has been located
            // on this firmware, so the screen could only ever open on
            // {"error":"not found"} with three switches that write nowhere.
            // The screen stays in the tree; see docs/MOBILE-API-GAP.md.
            //
            // The Quality section went with QoS for the same reason, rather
            // than being left as a header with nothing under it.

            Spacer(modifier = Modifier.height(8.dp))

            // System
            SectionHeader("System")
            SettingsItem(Icons.Default.SettingsPower, "Device Controls", onClick = onNavigateToDeviceControl)
            SettingsItem(Icons.Default.Schedule, "Scheduled Reboot", onClick = onNavigateToScheduleReboot)
        }
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(bottom = 4.dp),
    )
}

@Composable
private fun SettingsItem(
    icon: ImageVector,
    title: String,
    onClick: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
    ) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                icon,
                contentDescription = null,
                modifier = Modifier.size(24.dp),
                tint = MaterialTheme.colorScheme.primary,
            )
            Spacer(modifier = Modifier.width(16.dp))
            Text(
                title,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.weight(1f),
            )
            Icon(
                Icons.Default.ChevronRight,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
