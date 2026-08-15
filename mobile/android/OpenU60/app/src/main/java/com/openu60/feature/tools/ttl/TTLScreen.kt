package com.openu60.feature.tools.ttl

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TTLScreen(
    onBack: () -> Unit,
    viewModel: TTLViewModel = hiltViewModel(),
) {
    val state by viewModel.state.collectAsState()
    var input by remember { mutableStateOf("65") }

    LaunchedEffect(Unit) { viewModel.refresh() }
    // Seed the field from the router once its current value is known.
    LaunchedEffect(state.ttlValue) {
        if (state.ttlValue in 1..255) input = state.ttlValue.toString()
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("TTL Override") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
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

            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("TTL override", style = MaterialTheme.typography.titleMedium, fontWeight = androidx.compose.ui.text.font.FontWeight.Bold)
                    Text(
                        "Overrides the TTL / hop limit on LAN ingress traffic to prevent carrier tethering " +
                            "detection. Applied immediately and persists across reboots.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )

                    if (!state.loaded) {
                        Text("Checking status…", style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                    } else if (state.active) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Box(
                                modifier = Modifier.size(8.dp)
                                    .background(MaterialTheme.colorScheme.primary, shape = CircleShape),
                            )
                            Text("Active (TTL=${state.ttlValue})", style = MaterialTheme.typography.bodyMedium,
                                fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold,
                                color = MaterialTheme.colorScheme.primary)
                            if (state.ipv6Active) AssistChip(onClick = {}, enabled = false, label = { Text("IPv4 + IPv6") })
                        }
                    }

                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedTextField(
                            value = input,
                            onValueChange = { input = it.filter { c -> c.isDigit() }.take(3) },
                            label = { Text("TTL") },
                            singleLine = true,
                            enabled = !state.busy,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                            modifier = Modifier.width(120.dp),
                        )
                        Button(
                            onClick = { viewModel.setTtl(input.toIntOrNull() ?: 0) },
                            enabled = !state.busy && input.isNotBlank(),
                        ) { Text(if (state.active) "Update" else "Enable") }
                        if (state.active) {
                            OutlinedButton(onClick = { viewModel.clear() }, enabled = !state.busy) { Text("Disable") }
                        }
                    }
                }
            }
        }
    }
}
