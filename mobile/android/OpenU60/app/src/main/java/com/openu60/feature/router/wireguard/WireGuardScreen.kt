package com.openu60.feature.router.wireguard

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.*
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import com.openu60.core.model.WireguardParser
import com.openu60.core.model.WireguardProfile

/**
 * WireGuard, matching the dashboard's WireGuard tab.
 *
 * The private key is generated and stored on the router and never leaves it;
 * only the public half is shown. Saved tunnels hold a full provider `.conf`
 * each, so more than one provider can be kept even though the firmware has room
 * for a single active config.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WireGuardScreen(
    onBack: () -> Unit,
    viewModel: WireGuardViewModel = hiltViewModel(),
) {
    val state by viewModel.state.collectAsState()

    LaunchedEffect(Unit) { viewModel.refresh() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("WireGuard") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = state.isLoading,
            onRefresh = { viewModel.refresh() },
            modifier = Modifier.fillMaxSize().padding(padding),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                state.message?.let { msg ->
                    MessageCard(msg, state.messageIsError) { viewModel.clearMessage() }
                }

                if (!state.tunnel.available && !state.isLoading) {
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = MaterialTheme.colorScheme.errorContainer,
                        ),
                    ) {
                        Text(
                            "The wg tool is not installed on this router, so keys cannot be generated " +
                                "and the vendor tunnel scripts have nothing to call. Run scripts/zharden.sh, " +
                                "which installs it to /data/bin.",
                            modifier = Modifier.padding(16.dp),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onErrorContainer,
                        )
                    }
                    return@Column
                }

                SavedTunnelsCard(
                    profiles = state.profiles,
                    busy = state.busy,
                    onImport = { name, conf -> viewModel.importProfile(name, conf) },
                    onActivate = { viewModel.activateProfile(it) },
                    onDelete = { viewModel.deleteProfile(it) },
                )

                TunnelCard(state = state, onKeygen = { viewModel.keygen() }, onToggle = { viewModel.setConnected(it) })

                PeerCard(state = state, onEdit = viewModel::updateField, onSave = { viewModel.save() })
            }
        }
    }
}

@Composable
private fun MessageCard(message: String, isError: Boolean, onDismiss: () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = if (isError) MaterialTheme.colorScheme.errorContainer
            else MaterialTheme.colorScheme.primaryContainer,
        ),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(start = 16.dp, top = 12.dp, bottom = 12.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                message,
                modifier = Modifier.weight(1f),
                color = if (isError) MaterialTheme.colorScheme.onErrorContainer
                else MaterialTheme.colorScheme.onPrimaryContainer,
                style = MaterialTheme.typography.bodyMedium,
            )
            TextButton(onClick = onDismiss) { Text("Dismiss") }
        }
    }
}

@Composable
private fun SavedTunnelsCard(
    profiles: List<WireguardProfile>,
    busy: Boolean,
    onImport: (String, String) -> Unit,
    onActivate: (WireguardProfile) -> Unit,
    onDelete: (WireguardProfile) -> Unit,
) {
    var adding by remember { mutableStateOf(false) }
    var name by remember { mutableStateOf("") }
    var conf by remember { mutableStateOf("") }
    var toDelete by remember { mutableStateOf<WireguardProfile?>(null) }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Saved tunnels", style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
                TextButton(onClick = { adding = !adding }) { Text(if (adding) "Cancel" else "Import") }
            }

            if (adding) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Name") },
                    placeholder = { Text("e.g. Provider, Mumbai") },
                    singleLine = true,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = conf,
                    onValueChange = { conf = it },
                    label = { Text("Configuration") },
                    placeholder = { Text("[Interface]\nPrivateKey = …\nAddress = …\n\n[Peer]\nPublicKey = …\nEndpoint = host:port\nAllowedIPs = 0.0.0.0/0") },
                    enabled = !busy,
                    minLines = 6,
                    modifier = Modifier.fillMaxWidth(),
                    textStyle = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                )
                Text(
                    "Paste the provider's .conf exactly as given. DNS, MTU and keepalive lines are " +
                        "ignored — this firmware has nowhere to put them.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Button(
                    onClick = { onImport(name, conf); adding = false; name = ""; conf = "" },
                    enabled = !busy && conf.isNotBlank(),
                ) { Text("Save profile") }
            }

            if (profiles.isEmpty() && !adding) {
                Text("No saved tunnels. Import a provider's .conf to keep more than one.",
                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }

            profiles.forEach { p ->
                OutlinedCard(modifier = Modifier.fillMaxWidth()) {
                    Row(
                        modifier = Modifier.padding(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                Text(p.name, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold)
                                if (p.active) AssistChip(onClick = {}, enabled = false, label = { Text("Active") })
                            }
                            if (p.summary.isNotBlank()) {
                                Text(p.summary, style = MaterialTheme.typography.bodySmall,
                                    fontFamily = FontFamily.Monospace, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                        OutlinedButton(onClick = { onActivate(p) }, enabled = !busy && !p.active) {
                            Text(if (p.active) "In use" else "Use")
                        }
                        IconButton(onClick = { toDelete = p }, enabled = !busy) {
                            Icon(Icons.Default.Close, contentDescription = "Delete")
                        }
                    }
                }
            }
        }
    }

    toDelete?.let { profile ->
        AlertDialog(
            onDismissRequest = { toDelete = null },
            title = { Text("Delete ${profile.name}?") },
            text = {
                Text(
                    "The private key goes with it. Most providers issue a key once, so unless you still " +
                        "have the original .conf file this tunnel cannot be recreated.",
                )
            },
            confirmButton = { TextButton(onClick = { onDelete(profile); toDelete = null }) { Text("Delete") } },
            dismissButton = { TextButton(onClick = { toDelete = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun TunnelCard(state: WireGuardUiState, onKeygen: () -> Unit, onToggle: (Boolean) -> Unit) {
    var confirmKeygen by remember { mutableStateOf(false) }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Tunnel", style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
                AssistChip(
                    onClick = {},
                    enabled = false,
                    label = { Text(if (state.tunnel.connected) "Connected" else state.tunnel.connectStatus.ifBlank { "Not connected" }) },
                )
            }

            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(modifier = Modifier.weight(1f)) {
                    Text(if (state.tunnel.configured) "Key pair present" else "No key pair yet",
                        style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold)
                    Text(
                        if (state.tunnel.configured) "The private key is stored on the router and never sent to this screen."
                        else "Generate one before connecting. The private half stays on the router.",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                OutlinedButton(onClick = { confirmKeygen = true }, enabled = !state.busy) {
                    Text(if (state.tunnel.configured) "Regenerate" else "Generate")
                }
            }

            state.generatedPublicKey?.let { key ->
                OutlinedCard {
                    Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text("Public key", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold)
                        Text(key, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                        Text("Give this to the peer. The private half stays on the router.",
                            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }

            HorizontalDivider()

            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(modifier = Modifier.weight(1f)) {
                    Text("Connection", style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold)
                    Text(
                        when {
                            state.canConnect -> "Brings the tunnel up using the vendor scripts."
                            state.missingToConnect.isNotEmpty() -> "Still needed: ${state.missingToConnect.joinToString(", ")}"
                            else -> "Generate a key pair first."
                        },
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Switch(
                    checked = state.tunnel.connected,
                    onCheckedChange = onToggle,
                    enabled = !state.busy && (state.tunnel.connected || state.canConnect),
                )
            }
        }
    }

    if (confirmKeygen) {
        AlertDialog(
            onDismissRequest = { confirmKeygen = false },
            title = { Text(if (state.tunnel.configured) "Replace the existing key?" else "Generate a key pair?") },
            text = {
                Text(
                    if (state.tunnel.configured)
                        "The current private key is overwritten and cannot be recovered. Every peer configured " +
                            "with the old public key will stop accepting this router until you give them the new one."
                    else
                        "The private key stays on the router. Only the public key is shown, which is the half you " +
                            "give to the peer.",
                )
            },
            confirmButton = { TextButton(onClick = { confirmKeygen = false; onKeygen() }) { Text("Generate") } },
            dismissButton = { TextButton(onClick = { confirmKeygen = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun PeerCard(state: WireGuardUiState, onEdit: (String, String) -> Unit, onSave: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Peer and addressing", style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
                Button(onClick = onSave, enabled = !state.busy) { Text("Save") }
            }
            WireguardParser.fields.forEach { field ->
                OutlinedTextField(
                    value = state.draft[field.key] ?: "",
                    onValueChange = { onEdit(field.key, it) },
                    label = { Text(field.label) },
                    supportingText = field.hint?.let { { Text(it) } },
                    singleLine = true,
                    enabled = !state.busy,
                    modifier = Modifier.fillMaxWidth(),
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                )
            }
        }
    }
}
