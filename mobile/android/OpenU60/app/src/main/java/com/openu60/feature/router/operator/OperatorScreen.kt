package com.openu60.feature.router.operator

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import com.openu60.core.model.ScannedOperator

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OperatorScreen(
    onBack: () -> Unit,
    viewModel: OperatorViewModel = hiltViewModel(),
) {
    val state by viewModel.state.collectAsState()
    var confirmScan by remember { mutableStateOf(false) }
    var toRegister by remember { mutableStateOf<ScannedOperator?>(null) }

    LaunchedEffect(Unit) { viewModel.refresh() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Operator selection") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    TextButton(onClick = { viewModel.automatic() }, enabled = !state.busy && !state.scan.scanning) {
                        Text("Automatic")
                    }
                    Button(
                        onClick = { confirmScan = true },
                        enabled = !state.busy && !state.scan.scanning,
                        modifier = Modifier.padding(end = 8.dp),
                    ) { Text("Scan") }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
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
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, top = 12.dp, bottom = 12.dp, end = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(msg, modifier = Modifier.weight(1f),
                            color = if (state.messageIsError) MaterialTheme.colorScheme.onErrorContainer
                            else MaterialTheme.colorScheme.onPrimaryContainer,
                            style = MaterialTheme.typography.bodyMedium)
                        TextButton(onClick = { viewModel.clearMessage() }) { Text("Dismiss") }
                    }
                }
            }

            Text(
                "Normally the modem picks a network itself. Scanning shows which are actually " +
                    "reachable, and one can be chosen by hand — useful when the automatic choice is a " +
                    "weaker network, or to confirm a SIM is barred somewhere.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            if (state.scan.scanning) {
                Card {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                        Text("Sweeping the bands. About 40 seconds, and mobile data is down until it finishes.",
                            style = MaterialTheme.typography.bodyMedium)
                    }
                }
            } else if (state.scan.failed) {
                Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer)) {
                    Text(
                        "The scan failed. The modem reports this when it cannot sweep, most often because " +
                            "it has no usable service to begin with. Check the SIM is registered before trying again.",
                        modifier = Modifier.padding(16.dp),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onErrorContainer,
                    )
                }
            } else if (state.loaded && state.scan.operators.isEmpty()) {
                Text("No results yet. Run a scan to see which networks are in range.",
                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }

            state.scan.operators.forEach { op ->
                OperatorRow(op = op, busy = state.busy, onSelect = { toRegister = op })
            }
        }
    }

    if (confirmScan) {
        AlertDialog(
            onDismissRequest = { confirmScan = false },
            title = { Text("Scan for networks?") },
            text = {
                Text("The modem sweeps every band looking for operators, which takes about 40 seconds. " +
                    "Mobile data is interrupted for the whole scan.")
            },
            confirmButton = { TextButton(onClick = { confirmScan = false; viewModel.startScan() }) { Text("Scan") } },
            dismissButton = { TextButton(onClick = { confirmScan = false }) { Text("Cancel") } },
        )
    }

    toRegister?.let { op ->
        AlertDialog(
            onDismissRequest = { toRegister = null },
            title = { Text("Register on ${op.name}?") },
            text = {
                Text(
                    if (op.isForbidden)
                        "This network reports itself as forbidden for your SIM, so registration will very " +
                            "likely be refused and the router may be left without service until you switch back to automatic."
                    else
                        "The modem stops choosing a network for itself and stays on this one, even when the " +
                            "signal is poor. Switch back to automatic to undo it.",
                )
            },
            confirmButton = { TextButton(onClick = { viewModel.select(op); toRegister = null }) { Text("Register") } },
            dismissButton = { TextButton(onClick = { toRegister = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun OperatorRow(op: ScannedOperator, busy: Boolean, onSelect: () -> Unit) {
    OutlinedCard(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(op.name.ifBlank { "${op.mcc}-${op.mnc}" }, style = MaterialTheme.typography.bodyLarge,
                        fontWeight = FontWeight.SemiBold)
                    AssistChip(
                        onClick = {},
                        enabled = false,
                        label = { Text(op.status) },
                    )
                }
                Text("${op.mcc}-${op.mnc} · ${op.rat}", style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            OutlinedButton(onClick = onSelect, enabled = !busy && !op.isCurrent) {
                Text(if (op.isCurrent) "In use" else "Use")
            }
        }
    }
}
