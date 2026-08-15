package com.openu60.feature.router.esim

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.*
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import com.openu60.core.model.ActivationCode
import com.openu60.core.model.EuiccDownloadRequest
import com.openu60.core.model.EuiccProfile
import com.openu60.core.network.EuiccRelay
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.common.InputImage

/**
 * eSIM profile management, matching the dashboard's eSIM tab.
 *
 * EID and ICCID are masked until the user reveals them: they tie to a
 * subscriber and this screen is the kind of thing pasted into a bug report.
 * Every write goes through lpac on the router; if lpac is absent the agent
 * says so via capabilities and the whole management surface is hidden rather
 * than shown and then failing.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ESIMScreen(
    onBack: () -> Unit,
    viewModel: ESIMViewModel = hiltViewModel(),
) {
    val state by viewModel.state.collectAsState()
    val relayState by viewModel.relay.state.collectAsState()

    // Start refresh and, if relay is on, the carrier when the screen opens. The
    // relay is tied to the screen deliberately (see the ViewModel): a long poll
    // for the life of the app would cost battery for a feature used rarely.
    LaunchedEffect(Unit) { viewModel.refresh() }
    DisposableEffect(Unit) { onDispose { viewModel.stopRelay() } }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("eSIM") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    if (state.status.euiccAvailable) {
                        TextButton(onClick = { viewModel.toggleReveal() }) {
                            Text(if (state.revealed) "Hide" else "Show full")
                        }
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
                state.rebootNotice?.let { notice ->
                    RebootBanner(
                        notice = notice,
                        onReboot = { viewModel.reboot() },
                        onDismiss = { viewModel.dismissRebootNotice() },
                    )
                }

                state.message?.let { msg ->
                    MessageCard(msg, state.messageIsError) { viewModel.clearMessage() }
                }

                state.busyWith?.let { label ->
                    Card {
                        Row(
                            modifier = Modifier.fillMaxWidth().padding(16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                            Text("$label…", style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                }

                if (!state.status.euiccAvailable && !state.isLoading) {
                    Card(modifier = Modifier.fillMaxWidth()) {
                        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(
                                if (state.status.cardPresent) "No eUICC on this card" else "No card detected",
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.Bold,
                            )
                            if (state.status.detail.isNotBlank()) {
                                Text(state.status.detail, style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                    return@Column
                }

                EuiccCard(state = state, onReadChip = { viewModel.loadChipInfo() })

                if (state.canWrite) {
                    if (state.canRelay) {
                        RelayCard(
                            state = relayState,
                            onToggle = { viewModel.setUseRelay(it) },
                        )
                    }
                    AddProfileSection(
                        onDownload = { viewModel.download(it) },
                        busy = state.busyWith != null,
                        progress = state.progress,
                    )
                }

                ProfilesCard(
                    profiles = state.profiles,
                    canWrite = state.canWrite,
                    onEnable = { viewModel.enable(it) },
                    onDisable = { p, force -> viewModel.disable(p, force) },
                    onDelete = { viewModel.delete(it) },
                    onRename = { p, name -> viewModel.rename(p, name) },
                )

                if (state.canWrite && state.undeliverableNotifications.isNotEmpty()) {
                    NotificationsCard(
                        count = state.undeliverableNotifications.size,
                        onRetry = { viewModel.retryNotifications() },
                        onDiscard = {
                            viewModel.discardNotifications(
                                state.undeliverableNotifications.map { it.seqNumber },
                            )
                        },
                    )
                }

                if (!state.canWrite) {
                    Text(
                        "This agent has no lpac installed, so profiles can be read but not changed.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
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

/**
 * The switch happened on the card but the modem has not seen it.
 *
 * This modem rejects ES10c EnableProfile's refresh flag, so the profile list
 * is already correct while the network side still shows the old subscriber.
 * Saying nothing would make a successful operation look like a failed one.
 */
@Composable
private fun RebootBanner(notice: String, onReboot: () -> Unit, onDismiss: () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.tertiaryContainer,
        ),
    ) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(notice, style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onTertiaryContainer)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = onReboot) { Text("Reboot now") }
                TextButton(onClick = onDismiss) { Text("Later") }
            }
        }
    }
}

@Composable
private fun EuiccCard(state: ESIMState, onReadChip: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text("eUICC", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            Spacer(modifier = Modifier.height(8.dp))
            InfoRow("Status", if (state.status.euiccAvailable) "Detected" else "—")
            InfoRow("EID", state.eid ?: "—")
            InfoRow("Profiles", state.profiles.size.toString())
            state.chip?.let { chip ->
                InfoRow("Firmware", chip.firmwareVersion ?: "—")
                InfoRow("SGP.22 version", chip.profileVersion ?: "—")
                InfoRow("Free space", formatFreeSpace(chip.freeNonVolatileMemory))
            }
            if (state.canWrite && state.chip == null) {
                Spacer(modifier = Modifier.height(4.dp))
                TextButton(onClick = onReadChip, contentPadding = PaddingValues(0.dp)) {
                    Text("Read chip details")
                }
            }
        }
    }
}

@Composable
private fun RelayCard(state: EuiccRelay.State, onToggle: (Boolean) -> Unit) {
    var enabled by remember { mutableStateOf(false) }
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(modifier = Modifier.weight(1f)) {
                    Text("Router has no internet", style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold)
                    Text(
                        "This phone carries the operator traffic. Keep its mobile data on while it stays " +
                            "on the router's Wi-Fi. Needed to download a profile, and to report an enable, " +
                            "disable or delete back to the operator.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Switch(
                    checked = enabled,
                    onCheckedChange = { enabled = it; onToggle(it) },
                )
            }
            if (enabled) {
                val label = when (state) {
                    EuiccRelay.State.Idle -> "Relay idle"
                    EuiccRelay.State.Waiting -> "Relay ready, waiting for the router"
                    EuiccRelay.State.Carrying -> "Carrying operator traffic…"
                }
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (state == EuiccRelay.State.Carrying) {
                        CircularProgressIndicator(modifier = Modifier.size(12.dp), strokeWidth = 2.dp)
                    }
                    Text(label, style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.primary)
                }
            }
        }
    }
}

private enum class AddMode { Code, Manual }

@Composable
private fun AddProfileSection(
    onDownload: (EuiccDownloadRequest) -> Unit,
    busy: Boolean,
    progress: List<String>,
) {
    val context = LocalContext.current
    var open by remember { mutableStateOf(false) }
    var mode by remember { mutableStateOf(AddMode.Code) }

    var code by remember { mutableStateOf("") }
    var smdp by remember { mutableStateOf("") }
    var matchingId by remember { mutableStateOf("") }
    var confirmationCode by remember { mutableStateOf("") }
    var imei by remember { mutableStateOf("") }
    var scanError by remember { mutableStateOf<String?>(null) }

    val parsed = if (mode == AddMode.Code) ActivationCode.parse(code) else null
    val needsConfirmation = parsed?.confirmationCodeRequired == true
    val ready = if (mode == AddMode.Code) {
        parsed != null && (!needsConfirmation || confirmationCode.isNotBlank())
    } else {
        smdp.isNotBlank() && matchingId.isNotBlank()
    }

    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        scanError = null
        val image = runCatching { InputImage.fromFilePath(context, uri) }.getOrNull()
        if (image == null) {
            scanError = "Could not read that image."
            return@rememberLauncherForActivityResult
        }
        BarcodeScanning.getClient()
            .process(image)
            .addOnSuccessListener { barcodes ->
                val text = barcodes.firstNotNullOfOrNull { it.rawValue }
                if (text == null) {
                    scanError = "No QR code found in that image."
                } else if (ActivationCode.parse(text) == null) {
                    scanError = "That QR code is not an eSIM activation code."
                } else {
                    code = text
                }
            }
            .addOnFailureListener { scanError = it.message ?: "Could not scan that image." }
    }

    fun reset() {
        code = ""; smdp = ""; matchingId = ""; confirmationCode = ""; imei = ""; scanError = null
    }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Add a profile", style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
                if (!open) {
                    Button(onClick = { open = true }) { Text("Add") }
                } else {
                    TextButton(onClick = { reset(); open = false }) { Text("Close") }
                }
            }

            if (!open) {
                Text(
                    "Scan a QR code, paste an activation code, or type the details from your operator.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                return@Column
            }

            SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
                SegmentedButton(
                    selected = mode == AddMode.Code,
                    onClick = { mode = AddMode.Code },
                    shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
                ) { Text("Activation code") }
                SegmentedButton(
                    selected = mode == AddMode.Manual,
                    onClick = { mode = AddMode.Manual },
                    shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
                ) { Text("Enter manually") }
            }

            if (mode == AddMode.Code) {
                OutlinedTextField(
                    value = code,
                    onValueChange = { code = it },
                    label = { Text("Activation code") },
                    placeholder = { Text("LPA:1\$rsp.example.com\$ABC-123") },
                    singleLine = true,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                )
                OutlinedButton(onClick = { picker.launch("image/*") }, enabled = !busy) {
                    Text("Scan QR image")
                }
                parsed?.let {
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        AssistChip(onClick = {}, label = { Text("Valid code") }, enabled = false)
                        InfoRow("SM-DP+", it.smdp)
                        InfoRow("Matching ID", it.matchingId)
                    }
                }
            } else {
                OutlinedTextField(
                    value = smdp,
                    onValueChange = { smdp = it },
                    label = { Text("SM-DP+ address") },
                    placeholder = { Text("rsp.example.com") },
                    singleLine = true,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                )
                OutlinedTextField(
                    value = matchingId,
                    onValueChange = { matchingId = it },
                    label = { Text("Matching ID") },
                    placeholder = { Text("ABC-123-DEF") },
                    singleLine = true,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                )
            }

            if (mode == AddMode.Manual || needsConfirmation) {
                OutlinedTextField(
                    value = confirmationCode,
                    onValueChange = { confirmationCode = it },
                    label = { Text("Confirmation code") },
                    visualTransformation = PasswordVisualTransformation(),
                    singleLine = true,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                )
            }

            if (mode == AddMode.Manual) {
                OutlinedTextField(
                    value = imei,
                    onValueChange = { imei = it },
                    label = { Text("IMEI (optional)") },
                    placeholder = { Text("Leave blank to use the router's") },
                    singleLine = true,
                    enabled = !busy,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.fillMaxWidth(),
                )
            }

            scanError?.let {
                Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }

            if (progress.isNotEmpty()) {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    progress.takeLast(6).forEach { step ->
                        Text("• $step", style = MaterialTheme.typography.bodySmall,
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }

            Button(
                onClick = {
                    val request = if (mode == AddMode.Code && parsed != null) {
                        EuiccDownloadRequest(
                            smdp = parsed.smdp,
                            matchingId = parsed.matchingId,
                            confirmationCode = confirmationCode.ifBlank { null },
                        )
                    } else {
                        EuiccDownloadRequest(
                            smdp = smdp.trim(),
                            matchingId = matchingId.trim(),
                            confirmationCode = confirmationCode.ifBlank { null },
                            imei = imei.ifBlank { null },
                        )
                    }
                    onDownload(request)
                    reset(); open = false
                },
                enabled = ready && !busy,
            ) { Text("Download profile") }
            if (busy) {
                Text(
                    "This talks to your operator and can take a minute.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ProfilesCard(
    profiles: List<EuiccProfile>,
    canWrite: Boolean,
    onEnable: (EuiccProfile) -> Unit,
    onDisable: (EuiccProfile, Boolean) -> Unit,
    onDelete: (EuiccProfile) -> Unit,
    onRename: (EuiccProfile, String) -> Unit,
) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("Profiles", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            if (profiles.isEmpty()) {
                Text(
                    "No profiles installed. This eUICC is commissioned but carries none.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                val enabledCount = profiles.count { it.enabled }
                profiles.forEach { profile ->
                    ProfileRow(
                        profile = profile,
                        canWrite = canWrite,
                        isOnlyEnabled = profile.enabled && enabledCount == 1,
                        onEnable = { onEnable(profile) },
                        onDisable = { force -> onDisable(profile, force) },
                        onDelete = { onDelete(profile) },
                        onRename = { name -> onRename(profile, name) },
                    )
                }
            }
        }
    }
}

@Composable
private fun ProfileRow(
    profile: EuiccProfile,
    canWrite: Boolean,
    isOnlyEnabled: Boolean,
    onEnable: () -> Unit,
    onDisable: (Boolean) -> Unit,
    onDelete: () -> Unit,
    onRename: (String) -> Unit,
) {
    var renaming by remember { mutableStateOf(false) }
    var confirmDisable by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }
    val actionable = canWrite && (profile.iccid != null || profile.isdpAid != null)

    OutlinedCard(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(profile.displayName, style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
                AssistChip(
                    onClick = {},
                    enabled = false,
                    label = { Text(profile.state) },
                )
            }
            profile.serviceProvider?.let { InfoRow("Provider", it) }
            InfoRow("ICCID", profile.iccid ?: "—")
            profile.isdpAid?.let { InfoRow("ISD-P AID", it) }
            InfoRow("Class", profile.profileClass)

            if (actionable) {
                Spacer(modifier = Modifier.height(4.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    if (profile.enabled) {
                        OutlinedButton(onClick = { confirmDisable = true }) { Text("Disable") }
                    } else {
                        Button(onClick = onEnable) { Text("Enable") }
                    }
                    OutlinedButton(onClick = { renaming = true }) {
                        Icon(Icons.Default.Edit, contentDescription = "Rename", modifier = Modifier.size(16.dp))
                    }
                    OutlinedButton(
                        onClick = { confirmDelete = true },
                        enabled = !profile.enabled,
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
                    ) {
                        Icon(Icons.Default.Delete, contentDescription = "Delete", modifier = Modifier.size(16.dp))
                    }
                }
            }
        }
    }

    if (renaming) {
        RenameDialog(
            current = profile.nickname.orEmpty(),
            onDismiss = { renaming = false },
            onConfirm = { renaming = false; onRename(it) },
        )
    }
    if (confirmDisable) {
        AlertDialog(
            onDismissRequest = { confirmDisable = false },
            title = { Text("Disable ${profile.displayName}?") },
            text = {
                Text(
                    if (isOnlyEnabled)
                        "This is the only enabled profile. The router will have no mobile service until you enable another one."
                    else
                        "The profile stays on the card and can be enabled again.",
                )
            },
            confirmButton = {
                TextButton(onClick = { confirmDisable = false; onDisable(isOnlyEnabled) }) { Text("Disable") }
            },
            dismissButton = { TextButton(onClick = { confirmDisable = false }) { Text("Cancel") } },
        )
    }
    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("Delete ${profile.displayName}?") },
            text = {
                Text(
                    "This erases the profile from the card. The operator is told, which is what lets some " +
                        "of them release the activation code for reuse. Many issue single-use codes, so treat " +
                        "it as permanent.",
                )
            },
            confirmButton = {
                TextButton(onClick = { confirmDelete = false; onDelete() }) { Text("Delete") }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun RenameDialog(current: String, onDismiss: () -> Unit, onConfirm: (String) -> Unit) {
    var value by remember { mutableStateOf(current) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Rename profile") },
        text = {
            OutlinedTextField(
                value = value,
                onValueChange = { value = it },
                label = { Text("Nickname (blank to clear)") },
                singleLine = true,
            )
        },
        confirmButton = { TextButton(onClick = { onConfirm(value.trim()) }) { Text("Save") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun NotificationsCard(count: Int, onRetry: () -> Unit, onDiscard: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text("Undelivered notifications", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            Text(
                "$count notification${if (count == 1) "" else "s"} could not be delivered to the operator. " +
                    "Connect a relay and retry, or discard if the operator's server is gone.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = onRetry) {
                    Icon(Icons.Default.Refresh, contentDescription = null, modifier = Modifier.size(16.dp))
                    Spacer(modifier = Modifier.width(4.dp))
                    Text("Retry")
                }
                OutlinedButton(onClick = onDiscard) { Text("Discard") }
            }
        }
    }
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value.ifBlank { "—" }, style = MaterialTheme.typography.bodyMedium, fontFamily = FontFamily.Monospace)
    }
}

private fun formatFreeSpace(bytes: Long?): String {
    if (bytes == null) return "—"
    return if (bytes >= 1024 * 1024) "%.2f MB free".format(bytes / 1024.0 / 1024.0)
    else "${bytes / 1024} KB free"
}
