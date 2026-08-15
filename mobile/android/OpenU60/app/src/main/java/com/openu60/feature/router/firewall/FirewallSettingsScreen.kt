package com.openu60.feature.router.firewall

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.*
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FirewallSettingsScreen(
    onBack: () -> Unit,
    onNavigateToPortForwardForm: () -> Unit = {},
    viewModel: FirewallSettingsViewModel = hiltViewModel(),
) {
    val state by viewModel.state.collectAsState()

    LaunchedEffect(Unit) { viewModel.refresh() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Firewall") },
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
                    Card(
                        colors = CardDefaults.cardColors(
                            containerColor = if (state.messageIsError) MaterialTheme.colorScheme.errorContainer
                            else MaterialTheme.colorScheme.primaryContainer,
                        ),
                    ) {
                        Text(
                            msg,
                            modifier = Modifier.padding(16.dp),
                            color = if (state.messageIsError) MaterialTheme.colorScheme.onErrorContainer
                            else MaterialTheme.colorScheme.onPrimaryContainer,
                        )
                    }
                }

                // Firewall toggles
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text("General", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
                        Spacer(modifier = Modifier.height(8.dp))
                        ToggleRow("Firewall", state.config.enabled) { viewModel.toggleFirewall(it) }
                        ToggleRow("Port Forwarding", state.config.portForwardEnabled) { viewModel.togglePortForward(it) }
                        ToggleRow("Port Mapping (UPnP)", state.config.portMappingEnabled) { viewModel.togglePortMapping(it) }
                    }
                }

                // Reported but not writable.
                //
                // The agent reads these and has no setter for any of them, so
                // they are shown as status rather than as switches that would
                // flip back on the next refresh. The two inbound paths are named
                // plainly: "remote web access" reads harmless and is not.
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text("Read-only", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
                        Spacer(modifier = Modifier.height(8.dp))
                        StatusRow("NAT", state.config.nat)
                        StatusRow("Admin reachable from the internet", state.config.remoteAdminEnabled)
                        StatusRow("Answers pings from the internet", state.config.wanPingEnabled)
                        StatusRow("DMZ", state.config.dmzEnabled)
                        if (state.config.dmzEnabled && state.config.dmzHost.isNotBlank()) {
                            Text(
                                "DMZ host: ${state.config.dmzHost}",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            "Change these in the vendor web interface. The firmware exposes no setter for them.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }

                // Port Forward Rules
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text("Port Forward Rules", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
                            IconButton(onClick = { viewModel.showAddForm() }) {
                                Icon(Icons.Default.Add, contentDescription = "Add rule")
                            }
                        }
                        if (state.portForwardRules.isEmpty()) {
                            Text("No rules configured", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        } else {
                            state.portForwardRules.forEach { rule ->
                                Row(
                                    modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Column(modifier = Modifier.weight(1f)) {
                                        Text(rule.name.ifBlank { "Unnamed" }, fontWeight = FontWeight.Medium)
                                        Text(
                                            "${rule.protocol.uppercase()} WAN:${rule.wanPort} -> ${rule.lanIP}:${rule.lanPort}",
                                            style = MaterialTheme.typography.bodySmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                    IconButton(onClick = { viewModel.deletePortForwardRule(rule.id) }) {
                                        Icon(Icons.Default.Delete, contentDescription = "Delete", tint = MaterialTheme.colorScheme.error)
                                    }
                                }
                                HorizontalDivider()
                            }
                        }
                    }
                }

                // Inline port forward form
                if (state.showPortForwardForm) {
                    PortForwardFormInline(
                        onSubmit = { name, protocol, wanPort, lanIP, lanPort ->
                            viewModel.addPortForwardRule(name, protocol, wanPort, lanIP, lanPort, true)
                        },
                        onCancel = { viewModel.hideAddForm() },
                        isLoading = state.isLoading,
                    )
                }
            }
        }
    }
}

@Composable
private fun ToggleRow(label: String, checked: Boolean, onToggle: (Boolean) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge)
        Switch(checked = checked, onCheckedChange = onToggle)
    }
}

@Composable
private fun StatusRow(label: String, on: Boolean) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge)
        Text(
            if (on) "On" else "Off",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun PortForwardFormInline(
    onSubmit: (String, String, String, String, String) -> Unit,
    onCancel: () -> Unit,
    isLoading: Boolean,
) {
    var name by remember { mutableStateOf("") }
    var protocol by remember { mutableStateOf("tcp") }
    var wanPort by remember { mutableStateOf("") }
    var lanIP by remember { mutableStateOf("") }
    var lanPort by remember { mutableStateOf("") }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Add Port Forward Rule", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            OutlinedTextField(value = name, onValueChange = { name = it }, label = { Text("Name") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("tcp", "udp", "both").forEach { proto ->
                    FilterChip(
                        selected = protocol == proto,
                        onClick = { protocol = proto },
                        label = { Text(proto.uppercase()) },
                    )
                }
            }
            OutlinedTextField(value = wanPort, onValueChange = { wanPort = it }, label = { Text("WAN Port") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
            OutlinedTextField(value = lanIP, onValueChange = { lanIP = it }, label = { Text("LAN IP") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
            OutlinedTextField(value = lanPort, onValueChange = { lanPort = it }, label = { Text("LAN Port") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = { onSubmit(name, protocol, wanPort, lanIP, lanPort) },
                    enabled = !isLoading && name.isNotBlank() && wanPort.isNotBlank() && lanIP.isNotBlank() && lanPort.isNotBlank(),
                ) { Text("Add") }
                OutlinedButton(onClick = onCancel) { Text("Cancel") }
            }
        }
    }
}
