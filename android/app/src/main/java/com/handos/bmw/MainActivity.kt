package com.handos.bmw

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.handos.bmw.ui.theme.HandosTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import kotlin.math.sin

data class CodingItemUi(
    val id: String,
    val family: String,
    val ecu: String,
    val title: String,
    val description: String,
    val paramName: String,
    val onValue: String,
    val offValue: String,
)

data class VehicleUi(
    val vin: String,
    val chassis: String,
    val platformFamily: String,
    val batteryVoltage: Double,
    val ecus: Set<String>,
)

data class DtcUi(
    val ecu: String,
    val code: String,
    val description: String,
    val active: Boolean,
)

data class BackupUi(
    val ecu: String,
    val timestamp: String,
    val path: String,
)

data class TelemetryUi(
    val timestampMs: Long,
    val batteryVoltage: Double,
    val rpm: Int,
    val coolantTempC: Double,
    val oilTempC: Double,
    val boostKpa: Double,
    val intakeTempC: Double,
    val speedKmh: Double,
)

data class ExpertParamUi(
    val ecu: String,
    val name: String,
    val value: String,
)

enum class StepState { Pending, Active, Done, Failed }

data class CodingFlowStep(
    val label: String,
    val state: StepState = StepState.Pending,
)

data class CodingDialogState(
    val title: String,
    val ecu: String,
    val steps: List<CodingFlowStep>,
    val diffs: List<String> = emptyList(),
    val backupPath: String? = null,
    val error: String? = null,
    val finished: Boolean = false,
)

data class PendingConfirmation(
    val item: CodingItemUi,
    val enable: Boolean,
)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val backupDir = filesDir.resolve("backups").absolutePath
        val logsDir = filesDir.resolve("logs").absolutePath
        setContent {
            val systemDark = isSystemInDarkTheme()
            var darkTheme by remember { mutableStateOf(systemDark) }
            HandosTheme(darkTheme = darkTheme) {
                HandosApp(
                    backupDir = backupDir,
                    logsDir = logsDir,
                    darkTheme = darkTheme,
                    onToggleTheme = { darkTheme = !darkTheme },
                    onBindWifi = { MhdWifiBinder.bindProcessToWifi(this) },
                )
            }
        }
    }
}

private fun inferPlatformFamily(chassis: String): String {
    val upper = chassis.uppercase()
    return when {
        upper.contains("E46") -> "E46"
        upper.contains("E8") || upper.contains("E9") -> "E8xE9x"
        upper.contains("F1") || upper.contains("F2") || upper.contains("F3") || upper.contains("F8") -> "Fxx"
        else -> "GENERIC"
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HandosApp(
    backupDir: String,
    logsDir: String,
    darkTheme: Boolean,
    onToggleTheme: () -> Unit,
    onBindWifi: () -> Boolean,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var selectedTab by remember { mutableIntStateOf(0) }
    var adapterMode by remember { mutableStateOf("mhd_wifi") }
    var host by remember { mutableStateOf(HandosBridge.DEFAULT_MHD_HOST) }
    var statusText by remember { mutableStateOf("Connect via MHD WiFi or K+DCAN USB, or run Demo Simulation.") }
    var busy by remember { mutableStateOf(false) }
    var demoMode by remember { mutableStateOf(false) }
    var scannedVehicle by remember { mutableStateOf<VehicleUi?>(null) }
    var showAllVehicles by remember { mutableStateOf(false) }
    var expertMode by remember { mutableStateOf(false) }
    var expertSearch by remember { mutableStateOf("") }
    var expertParams by remember { mutableStateOf<List<ExpertParamUi>>(emptyList()) }
    var enabledPresetIds by remember { mutableStateOf<Set<String>>(emptySet()) }
    var backups by remember { mutableStateOf<List<BackupUi>>(emptyList()) }
    var dtcs by remember { mutableStateOf<List<DtcUi>>(emptyList()) }
    var pendingConfirm by remember { mutableStateOf<PendingConfirmation?>(null) }
    var codingDialog by remember { mutableStateOf<CodingDialogState?>(null) }

    // Live telemetry & logging state
    var isLogging by remember { mutableStateOf(false) }
    var latestTelemetry by remember { mutableStateOf<TelemetryUi?>(null) }
    val recordedSamples = remember { mutableStateListOf<TelemetryUi>() }

    val codingItems = remember {
        listOf(
            CodingItemUi("e46_usdm_corners", "E46", "LSZ", "USDM Corner Lights", "Front turn signals as sidemarkers", "SIDEMARKER_US", "aktiv", "nicht_aktiv"),
            CodingItemUi("e46_double_hazard", "E46", "LSZ", "Double Hazard Lights", "Double-impulse hazard flash", "BLINKIMPULS_WARNBLK", "wert_02", "wert_01"),
            CodingItemUi("e46_comfort_windows", "E46", "GM5", "Comfort Windows", "Hold fob to open/close windows", "KOMFORTSCHLIESSUNG_FB", "aktiv", "nicht_aktiv"),
            CodingItemUi("e8x_e9x_usdm_corners", "E8xE9x", "FRM", "USDM Corner Lights", "Dimmed front turn signals as sidemarkers", "SIDEMARKER_US", "aktiv", "nicht_aktiv"),
            CodingItemUi("e8x_e9x_double_hazard", "E8xE9x", "FRM", "Double Hazard Lights", "Double-impulse hazard blink", "DOPPELBLINKIMPULS_WARNBLK", "aktiv", "nicht_aktiv"),
            CodingItemUi("e8x_e9x_start_stop_memory", "E8xE9x", "IHKA", "Auto Start/Stop Memory", "Remember last Start/Stop state", "MSA_MEMORY", "aktiv", "nicht_aktiv"),
            CodingItemUi("e8x_e9x_digital_speedo", "E8xE9x", "KOMBI", "Digital Speedometer", "Uncorrected digital speed in cluster", "BC_DIGITAL_V", "aktiv", "nicht_aktiv"),
            CodingItemUi("fxx_start_stop_memory", "Fxx", "FEM_BODY", "Auto Start/Stop Memory", "Remember last Start/Stop state across restarts", "TCM_MSA_MEMORY", "aktiv", "nicht_aktiv"),
            CodingItemUi("fxx_start_stop_off_default", "Fxx", "FEM_BODY", "Auto Start/Stop Off by Default", "Default Start/Stop to OFF on every start", "TCM_MSA_DEFAULT_OFF", "aktiv", "nicht_aktiv"),
            CodingItemUi("fxx_digital_speedo", "Fxx", "KOMBI", "Digital Speedometer", "Show digital speed readout in cluster", "BC_DIGITAL_V", "aktiv", "nicht_aktiv"),
            CodingItemUi("fxx_mirrors_fold_lock", "Fxx", "FEM_BODY", "Fold Mirrors on Lock", "Fold side mirrors on key fob / comfort lock", "ASP_BEIKLAPPEN_BEI_KOMFORTSCHLIESSEN", "aktiv", "nicht_aktiv"),
            CodingItemUi("fxx_usdm_sidemarkers", "Fxx", "FEM_BODY", "USDM Corner Sidemarkers", "Enable US-spec front amber sidemarkers", "SIDEMARKER_US", "aktiv", "nicht_aktiv"),
            CodingItemUi("fxx_video_in_motion", "Fxx", "HU_NBT", "Video in Motion", "Remove iDrive video speed lock", "SPEEDLOCK_X_KMH_MAX", "werte=FF", "werte=03"),
            CodingItemUi("fxx_nbt_evo_aa_prep", "Fxx", "HU_NBT", "NBT EVO Android Auto Prep", "Enable CarLife slot & WLAN flags for NBT EVO AA", "CARLIFE", "aktiv", "nicht_aktiv"),
            CodingItemUi("fxx_nbt_evo_carplay_fullscreen", "Fxx", "HU_NBT", "NBT EVO CarPlay Fullscreen", "Enable fullscreen CarPlay on NBT EVO ID5/ID6", "CARPLAY_FULLSCREEN", "aktiv", "nicht_aktiv"),
        )
    }

    val visibleCodingItems = remember(codingItems, scannedVehicle, showAllVehicles) {
        val vehicle = scannedVehicle
        if (vehicle == null || showAllVehicles) {
            codingItems
        } else {
            codingItems.filter { item ->
                item.family == vehicle.platformFamily &&
                    (vehicle.ecus.isEmpty() || vehicle.ecus.contains(item.ecu))
            }
        }
    }

    // Live telemetry polling loop when logging is active
    LaunchedEffect(isLogging, demoMode) {
        var tick = 0
        while (isLogging) {
            val sample = if (demoMode) {
                tick++
                val wave = sin(tick * 0.4)
                TelemetryUi(
                    timestampMs = System.currentTimeMillis(),
                    batteryVoltage = 12.7 + wave * 0.05,
                    rpm = (2200 + wave * 900).toInt(),
                    coolantTempC = 91.5 + wave * 1.2,
                    oilTempC = 102.0 + wave * 1.5,
                    boostKpa = 145.0 + wave * 45.0,
                    intakeTempC = 33.0 + wave * 2.0,
                    speedKmh = (65.0 + wave * 20.0).coerceAtLeast(0.0),
                )
            } else if (HandosBridge.isNativeLoaded) {
                val raw = withContext(Dispatchers.IO) {
                    HandosBridge.nativeReadTelemetry(host, backupDir)
                }
                val json = JSONObject(raw)
                if (json.optBoolean("ok", false)) {
                    val d = json.getJSONObject("data")
                    TelemetryUi(
                        timestampMs = d.optLong("timestamp_ms"),
                        batteryVoltage = d.optDouble("battery_voltage"),
                        rpm = d.optInt("rpm"),
                        coolantTempC = d.optDouble("coolant_temp_c"),
                        oilTempC = d.optDouble("oil_temp_c"),
                        boostKpa = d.optDouble("boost_kpa"),
                        intakeTempC = d.optDouble("intake_temp_c"),
                        speedKmh = d.optDouble("vehicle_speed_kmh"),
                    )
                } else {
                    null
                }
            } else {
                null
            }
            if (sample != null) {
                latestTelemetry = sample
                recordedSamples.add(sample)
            }
            delay(700)
        }
    }

    fun startDemoSimulation() {
        demoMode = true
        val demoVehicle = VehicleUi(
            vin = "WBA3B1C50DF123456",
            chassis = "F30 (3-Series) [DEMO]",
            platformFamily = "Fxx",
            batteryVoltage = 12.74,
            ecus = setOf("FEM_BODY", "KOMBI", "HU_NBT"),
        )
        scannedVehicle = demoVehicle
        enabledPresetIds = setOf("fxx_digital_speedo", "fxx_nbt_evo_carplay_fullscreen")
        backups = listOf(
            BackupUi("FEM_BODY", "20261124_180000", "$backupDir/${demoVehicle.vin}/FEM_BODY_demo.trc"),
            BackupUi("KOMBI", "20261124_180000", "$backupDir/${demoVehicle.vin}/KOMBI_demo.trc"),
        )
        expertParams = listOf(
            ExpertParamUi("FEM_BODY", "TCM_MSA_MEMORY", "nicht_aktiv"),
            ExpertParamUi("FEM_BODY", "TCM_MSA_DEFAULT_OFF", "nicht_aktiv"),
            ExpertParamUi("FEM_BODY", "ASP_BEIKLAPPEN_BEI_KOMFORTSCHLIESSEN", "nicht_aktiv"),
            ExpertParamUi("KOMBI", "BC_DIGITAL_V", "aktiv"),
            ExpertParamUi("HU_NBT", "CARPLAY_FULLSCREEN", "aktiv"),
            ExpertParamUi("HU_NBT", "CARLIFE", "nicht_aktiv"),
        )
        dtcs = listOf(
            DtcUi("FEM_BODY", "8040B9", "Terminal 30B deactivation: startability limit reached", active = false),
            DtcUi("HU_NBT", "30FF", "Boost pressure below target (underboost)", active = false),
        )
        statusText = "Demo simulation active: ${demoVehicle.chassis} (${demoVehicle.vin})."
        selectedTab = 1
    }

    fun executeCodingFlow(item: CodingItemUi, enable: Boolean) {
        val actionLabel = if (enable) "Enabling" else "Turning off"
        val baseSteps = listOf(
            CodingFlowStep("Checking battery voltage & ${item.ecu} link", StepState.Active),
            CodingFlowStep("Reading current ${item.ecu} coding (FSW_PSW.TRC)"),
            CodingFlowStep("Saving automatic VIN backup"),
            CodingFlowStep("$actionLabel ${item.title} on ${item.ecu}"),
            CodingFlowStep("Re-reading & verifying changes"),
        )
        codingDialog = CodingDialogState(
            title = "${if (enable) "Enable" else "Turn Off"} ${item.title}",
            ecu = item.ecu,
            steps = baseSteps,
        )

        scope.launch {
            if (demoMode) {
                for (i in baseSteps.indices) {
                    delay(400)
                    codingDialog = codingDialog?.let { st ->
                        val updated = st.steps.mapIndexed { idx, s ->
                            when {
                                idx <= i -> s.copy(state = StepState.Done)
                                idx == i + 1 -> s.copy(state = StepState.Active)
                                else -> s
                            }
                        }
                        st.copy(steps = updated)
                    }
                }
                val vin = scannedVehicle?.vin ?: "DEMO_VIN"
                val fakeBackup = "$backupDir/$vin/${item.ecu}_demo.trc"
                val diffText = if (enable) {
                    "${item.paramName}: ${item.offValue} -> ${item.onValue}"
                } else {
                    "${item.paramName}: ${item.onValue} -> ${item.offValue}"
                }
                enabledPresetIds = if (enable) enabledPresetIds + item.id else enabledPresetIds - item.id
                backups = listOf(BackupUi(item.ecu, "just_now", fakeBackup)) + backups
                codingDialog = codingDialog?.copy(
                    diffs = listOf(diffText),
                    backupPath = fakeBackup,
                    finished = true,
                )
                statusText = "Demo: ${if (enable) "enabled" else "turned off"} ${item.title} (verified)."
                return@launch
            }

            if (!HandosBridge.isNativeLoaded) {
                codingDialog = codingDialog?.copy(
                    steps = baseSteps.mapIndexed { idx, s -> if (idx == 0) s.copy(state = StepState.Failed) else s },
                    error = "libhandos_core.so not loaded.",
                    finished = true,
                )
                return@launch
            }

            val raw = withContext(Dispatchers.IO) {
                if (enable) HandosBridge.nativeApplyPreset(host, item.id, backupDir)
                else HandosBridge.nativeRevertPreset(host, item.id, backupDir)
            }
            val json = JSONObject(raw)
            if (json.optBoolean("ok", false)) {
                val data = json.optJSONObject("data")
                val diffsArr = data?.optJSONArray("applied_diffs") ?: JSONArray()
                val diffs = List(diffsArr.length()) { i ->
                    val d = diffsArr.getJSONObject(i)
                    "${d.optString("parameter")}: ${d.optString("old_value")} -> ${d.optString("new_value")}"
                }
                val bPath = data?.optString("backup_path")
                enabledPresetIds = if (enable) enabledPresetIds + item.id else enabledPresetIds - item.id
                codingDialog = codingDialog?.copy(
                    steps = baseSteps.map { it.copy(state = StepState.Done) },
                    diffs = diffs,
                    backupPath = bPath,
                    finished = true,
                )
                statusText = "${if (enable) "Enabled" else "Turned off"} ${item.title} safely and verified."
            } else {
                val errMsg = json.optString("error", "Unknown error")
                codingDialog = codingDialog?.copy(
                    steps = baseSteps.mapIndexed { idx, s -> if (idx == 0) s.copy(state = StepState.Failed) else s },
                    error = errMsg,
                    finished = true,
                )
                statusText = "Error: $errMsg"
            }
        }
    }

    fun runNative(
        actionName: String,
        onSuccess: ((Any?) -> Unit)? = null,
        block: () -> String,
    ) {
        if (!HandosBridge.isNativeLoaded) {
            statusText = "$actionName: libhandos_core.so not bundled yet."
            return
        }
        scope.launch {
            busy = true
            statusText = "$actionName..."
            val raw = withContext(Dispatchers.IO) { block() }
            val json = JSONObject(raw)
            if (json.optBoolean("ok", false)) {
                val data = json.opt("data")
                onSuccess?.invoke(data)
                statusText = data?.toString() ?: "Done."
            } else {
                statusText = "Error: ${json.optString("error", "Unknown error")}"
            }
            busy = false
        }
    }

    pendingConfirm?.let { pending ->
        val diffPreview = if (pending.enable) {
            "${pending.item.paramName}: ${pending.item.offValue} -> ${pending.item.onValue}"
        } else {
            "${pending.item.paramName}: ${pending.item.onValue} -> ${pending.item.offValue}"
        }
        AlertDialog(
            onDismissRequest = { pendingConfirm = null },
            title = { Text(if (pending.enable) "Confirm Coding" else "Confirm Turn Off") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("${pending.item.title} (${pending.item.ecu})", fontWeight = FontWeight.SemiBold)
                    Text("Automatic VIN backup will be saved before writing:", style = MaterialTheme.typography.bodySmall)
                    Surface(shape = RoundedCornerShape(8.dp), color = MaterialTheme.colorScheme.surfaceVariant) {
                        Text(diffPreview, modifier = Modifier.padding(10.dp), style = MaterialTheme.typography.bodySmall)
                    }
                }
            },
            confirmButton = {
                Button(onClick = {
                    pendingConfirm = null
                    executeCodingFlow(pending.item, pending.enable)
                }) {
                    Text(if (pending.enable) "Backup & Enable" else "Backup & Turn Off")
                }
            },
            dismissButton = {
                TextButton(onClick = { pendingConfirm = null }) { Text("Cancel") }
            },
        )
    }

    codingDialog?.let { dlg ->
        CodingProgressDialog(state = dlg, onDismiss = { codingDialog = null })
    }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            TopAppBar(
                title = { Text(text = "handos", fontWeight = FontWeight.Bold) },
                actions = {
                    if (demoMode) {
                        AssistChip(
                            onClick = {
                                demoMode = false
                                isLogging = false
                                scannedVehicle = null
                                dtcs = emptyList()
                                backups = emptyList()
                                statusText = "Exited demo mode."
                            },
                            label = { Text("DEMO") },
                        )
                    }
                    IconButton(onClick = onToggleTheme) {
                        Icon(
                            imageVector = if (darkTheme) Icons.Rounded.LightMode else Icons.Rounded.DarkMode,
                            contentDescription = "Toggle theme",
                        )
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        bottomBar = {
            NavigationBar(containerColor = MaterialTheme.colorScheme.surface) {
                val tabs = listOf(
                    "Connect" to Icons.Rounded.Wifi,
                    "Coding" to Icons.Rounded.Tune,
                    "Logs" to Icons.Rounded.Speed,
                    "Faults" to Icons.Rounded.WarningAmber,
                )
                tabs.forEachIndexed { idx, (label, icon) ->
                    NavigationBarItem(
                        selected = selectedTab == idx,
                        onClick = { selectedTab = idx },
                        label = { Text(label) },
                        icon = { Icon(icon, contentDescription = label) },
                    )
                }
            }
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .padding(padding)
                .padding(horizontal = 16.dp, vertical = 8.dp)
                .fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Surface(
                shape = RoundedCornerShape(14.dp),
                color = MaterialTheme.colorScheme.surfaceVariant,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = statusText,
                    modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            AnimatedVisibility(visible = busy) {
                LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
            }

            when (selectedTab) {
                0 -> {
                    LazyColumn(
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                        contentPadding = PaddingValues(bottom = 12.dp),
                    ) {
                        item {
                            ElevatedCard(
                                shape = RoundedCornerShape(18.dp),
                                colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
                                modifier = Modifier.fillMaxWidth(),
                            ) {
                                Column(
                                    modifier = Modifier.padding(16.dp),
                                    verticalArrangement = Arrangement.spacedBy(12.dp),
                                ) {
                                    Text("Adapter Connection", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                                    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                        FilterChip(
                                            selected = adapterMode == "mhd_wifi",
                                            onClick = { adapterMode = "mhd_wifi" },
                                            label = { Text("MHD WiFi") },
                                            modifier = Modifier.weight(1f),
                                        )
                                        FilterChip(
                                            selected = adapterMode == "kdcan_usb",
                                            onClick = { adapterMode = "kdcan_usb" },
                                            label = { Text("K+DCAN USB (OTG)") },
                                            modifier = Modifier.weight(1f),
                                        )
                                    }
                                    if (adapterMode == "mhd_wifi") {
                                        OutlinedTextField(
                                            value = host,
                                            onValueChange = { host = it },
                                            label = { Text("MHD WiFi Endpoint") },
                                            singleLine = true,
                                            shape = RoundedCornerShape(12.dp),
                                            modifier = Modifier.fillMaxWidth(),
                                        )
                                    } else {
                                        Text(
                                            "Connect a standard FTDI K+DCAN cable via USB-C OTG.",
                                            style = MaterialTheme.typography.bodySmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                    Button(
                                        enabled = !busy,
                                        shape = RoundedCornerShape(14.dp),
                                        modifier = Modifier.fillMaxWidth().height(50.dp),
                                        onClick = {
                                            demoMode = false
                                            if (adapterMode == "kdcan_usb") {
                                                statusText = KdcanUsbManager.probeOrRequestPermission(context).message
                                                return@Button
                                            }
                                            onBindWifi()
                                            runNative(
                                                "Scanning vehicle",
                                                onSuccess = { data ->
                                                    val obj = data as? JSONObject ?: return@runNative
                                                    val chassis = obj.optString("chassis")
                                                    val ecuArr = obj.optJSONArray("ecus") ?: JSONArray()
                                                    val ecus = buildSet {
                                                        for (i in 0 until ecuArr.length()) {
                                                            add(ecuArr.getJSONObject(i).optString("name"))
                                                        }
                                                    }
                                                    val v = VehicleUi(
                                                        vin = obj.optString("vin"),
                                                        chassis = chassis,
                                                        platformFamily = inferPlatformFamily(chassis),
                                                        batteryVoltage = obj.optDouble("battery_voltage", 0.0),
                                                        ecus = ecus,
                                                    )
                                                    scannedVehicle = v
                                                    scope.launch {
                                                        val sRaw = withContext(Dispatchers.IO) { HandosBridge.nativePresetStatuses(host, backupDir) }
                                                        val sJson = JSONObject(sRaw)
                                                        if (sJson.optBoolean("ok", false)) {
                                                            val arr = sJson.optJSONArray("data") ?: JSONArray()
                                                            enabledPresetIds = buildSet {
                                                                for (i in 0 until arr.length()) {
                                                                    val entry = arr.getJSONObject(i)
                                                                    if (entry.optBoolean("enabled", false)) add(entry.optString("id"))
                                                                }
                                                            }
                                                        }
                                                        val bRaw = withContext(Dispatchers.IO) { HandosBridge.nativeListBackups(v.vin, backupDir) }
                                                        val bJson = JSONObject(bRaw)
                                                        if (bJson.optBoolean("ok", false)) {
                                                            val arr = bJson.optJSONArray("data") ?: JSONArray()
                                                            backups = List(arr.length()) { i ->
                                                                val entry = arr.getJSONObject(i)
                                                                BackupUi(entry.optString("ecu"), entry.optString("timestamp"), entry.optString("path"))
                                                            }
                                                        }
                                                    }
                                                },
                                            ) { HandosBridge.nativeScanVehicle(host, backupDir) }
                                        },
                                    ) {
                                        Icon(Icons.Rounded.DirectionsCar, contentDescription = null)
                                        Spacer(Modifier.width(8.dp))
                                        Text("Connect & Scan Vehicle")
                                    }
                                    OutlinedButton(
                                        enabled = !busy,
                                        shape = RoundedCornerShape(14.dp),
                                        modifier = Modifier.fillMaxWidth().height(48.dp),
                                        onClick = { startDemoSimulation() },
                                    ) {
                                        Icon(Icons.Rounded.PlayArrow, contentDescription = null)
                                        Spacer(Modifier.width(8.dp))
                                        Text("Run Demo Simulation (F30)")
                                    }
                                }
                            }
                        }

                        scannedVehicle?.let { v ->
                            item {
                                ElevatedCard(
                                    shape = RoundedCornerShape(18.dp),
                                    colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                        Row(
                                            modifier = Modifier.fillMaxWidth(),
                                            horizontalArrangement = Arrangement.SpaceBetween,
                                            verticalAlignment = Alignment.CenterVertically,
                                        ) {
                                            Text(v.chassis, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
                                            AssistChip(
                                                onClick = {},
                                                label = {
                                                    val warn = if (v.batteryVoltage < 12.0) " (LOW)" else ""
                                                    Text("${"%.2f".format(v.batteryVoltage)} V$warn")
                                                },
                                            )
                                        }
                                        Text("VIN: ${v.vin}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                        LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                            items(v.ecus.toList()) { ecuName ->
                                                AssistChip(onClick = {}, label = { Text(ecuName) })
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        item {
                            ElevatedCard(
                                shape = RoundedCornerShape(18.dp),
                                colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
                                modifier = Modifier.fillMaxWidth(),
                            ) {
                                Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Text("Service & Maintenance", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                                        FilledTonalButton(
                                            modifier = Modifier.weight(1f),
                                            onClick = {
                                                if (demoMode) statusText = "Demo: Registered new battery in DME/CAS."
                                                else runNative("Registering battery") { HandosBridge.nativeRunServiceJob(host, "register_battery", backupDir) }
                                            },
                                        ) { Text("Register Battery") }
                                        FilledTonalButton(
                                            modifier = Modifier.weight(1f),
                                            onClick = {
                                                if (demoMode) statusText = "Demo: Oil service (CBS) reset."
                                                else runNative("Resetting oil service") { HandosBridge.nativeRunServiceJob(host, "reset_oil_service", backupDir) }
                                            },
                                        ) { Text("Reset Oil CBS") }
                                    }
                                }
                            }
                        }

                        if (backups.isNotEmpty()) {
                            item {
                                ElevatedCard(
                                    shape = RoundedCornerShape(18.dp),
                                    colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                        Text("ECU Backups & Restore", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                                        backups.take(5).forEach { b ->
                                            Row(
                                                modifier = Modifier.fillMaxWidth(),
                                                horizontalArrangement = Arrangement.SpaceBetween,
                                                verticalAlignment = Alignment.CenterVertically,
                                            ) {
                                                Column(modifier = Modifier.weight(1f)) {
                                                    Text(b.ecu, fontWeight = FontWeight.Medium)
                                                    Text(b.timestamp, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                                }
                                                OutlinedButton(onClick = {
                                                    if (demoMode) statusText = "Demo: Restored ${b.ecu} from ${b.timestamp}."
                                                    else runNative("Restoring ${b.ecu}") { HandosBridge.nativeRestoreBackup(host, b.ecu, b.path, backupDir) }
                                                }) { Text("Restore") }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                1 -> {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            FilterChip(
                                selected = !expertMode,
                                onClick = { expertMode = false },
                                label = { Text("Curated") },
                            )
                            FilterChip(
                                selected = expertMode,
                                onClick = { expertMode = true },
                                label = { Text("Expert FSW_PSW") },
                            )
                        }
                        if (!expertMode && scannedVehicle != null) {
                            FilterChip(
                                selected = !showAllVehicles,
                                onClick = { showAllVehicles = !showAllVehicles },
                                label = { Text(if (showAllVehicles) "All" else scannedVehicle!!.chassis.substringBefore(" ")) },
                            )
                        }
                    }

                    if (expertMode) {
                        OutlinedTextField(
                            value = expertSearch,
                            onValueChange = { expertSearch = it },
                            label = { Text("Search FSW_PSW parameter") },
                            singleLine = true,
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.fillMaxWidth(),
                        )
                        val filteredParams = expertParams.filter {
                            expertSearch.isBlank() || it.name.contains(expertSearch, ignoreCase = true) || it.ecu.contains(expertSearch, ignoreCase = true)
                        }
                        LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            items(filteredParams) { p ->
                                ElevatedCard(shape = RoundedCornerShape(14.dp), modifier = Modifier.fillMaxWidth()) {
                                    Row(
                                        modifier = Modifier.padding(12.dp).fillMaxWidth(),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        Column(modifier = Modifier.weight(1f)) {
                                            Text("${p.ecu} | ${p.name}", fontWeight = FontWeight.Medium)
                                            Text("Current: ${p.value}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                        }
                                        FilledTonalButton(onClick = {
                                            val nextVal = if (p.value == "aktiv") "nicht_aktiv" else "aktiv"
                                            if (demoMode) {
                                                expertParams = expertParams.map {
                                                    if (it.ecu == p.ecu && it.name == p.name) it.copy(value = nextVal) else it
                                                }
                                                statusText = "Demo Expert: ${p.name} -> $nextVal (backed up & verified)."
                                            } else {
                                                runNative("Setting ${p.name}") {
                                                    HandosBridge.nativeSetEcuParam(host, p.ecu, p.name, nextVal, backupDir)
                                                }
                                            }
                                        }) {
                                            Text(if (p.value == "aktiv") "Set nicht_aktiv" else "Set aktiv")
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        LazyColumn(
                            verticalArrangement = Arrangement.spacedBy(10.dp),
                            contentPadding = PaddingValues(bottom = 12.dp),
                        ) {
                            items(visibleCodingItems) { item ->
                                val isEnabled = enabledPresetIds.contains(item.id)
                                ElevatedCard(
                                    shape = RoundedCornerShape(16.dp),
                                    colors = CardDefaults.elevatedCardColors(containerColor = MaterialTheme.colorScheme.surface),
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                        Row(
                                            modifier = Modifier.fillMaxWidth(),
                                            horizontalArrangement = Arrangement.SpaceBetween,
                                            verticalAlignment = Alignment.CenterVertically,
                                        ) {
                                            Text(
                                                item.title,
                                                style = MaterialTheme.typography.titleMedium,
                                                fontWeight = FontWeight.SemiBold,
                                                modifier = Modifier.weight(1f),
                                            )
                                            Spacer(Modifier.width(8.dp))
                                            AssistChip(
                                                onClick = {},
                                                label = { Text(if (isEnabled) "Enabled | ${item.ecu}" else "${item.family} | ${item.ecu}") },
                                            )
                                        }
                                        Text(item.description, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                        Spacer(Modifier.height(4.dp))
                                        if (isEnabled) {
                                            OutlinedButton(
                                                enabled = !busy && codingDialog == null,
                                                shape = RoundedCornerShape(12.dp),
                                                onClick = { pendingConfirm = PendingConfirmation(item, enable = false) },
                                            ) {
                                                Icon(Icons.Rounded.CheckCircle, contentDescription = null, modifier = Modifier.size(18.dp))
                                                Spacer(Modifier.width(6.dp))
                                                Text("Backup & Turn Off")
                                            }
                                        } else {
                                            FilledTonalButton(
                                                enabled = !busy && codingDialog == null,
                                                shape = RoundedCornerShape(12.dp),
                                                onClick = { pendingConfirm = PendingConfirmation(item, enable = true) },
                                            ) {
                                                Icon(Icons.Rounded.Build, contentDescription = null, modifier = Modifier.size(18.dp))
                                                Spacer(Modifier.width(6.dp))
                                                Text("Backup & Enable")
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                2 -> {
                    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Button(
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1f),
                            onClick = { isLogging = !isLogging },
                        ) {
                            Icon(if (isLogging) Icons.Rounded.Stop else Icons.Rounded.PlayArrow, contentDescription = null)
                            Spacer(Modifier.width(6.dp))
                            Text(if (isLogging) "Stop Logging" else "Start Logging")
                        }
                        OutlinedButton(
                            enabled = recordedSamples.isNotEmpty(),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1f),
                            onClick = {
                                val dir = File(logsDir).apply { mkdirs() }
                                val outFile = File(dir, "handos_log_${System.currentTimeMillis()}.csv")
                                val csv = buildString {
                                    appendLine("timestamp_ms,battery_voltage,rpm,coolant_temp_c,oil_temp_c,boost_kpa,intake_temp_c,vehicle_speed_kmh")
                                    recordedSamples.forEach { s ->
                                        appendLine("${s.timestampMs},${"%.2f".format(s.batteryVoltage)},${s.rpm},${"%.1f".format(s.coolantTempC)},${"%.1f".format(s.oilTempC)},${"%.1f".format(s.boostKpa)},${"%.1f".format(s.intakeTempC)},${"%.1f".format(s.speedKmh)}")
                                    }
                                }
                                outFile.writeText(csv)
                                statusText = "Saved ${recordedSamples.size} samples to ${outFile.name}"
                            },
                        ) {
                            Icon(Icons.Rounded.Save, contentDescription = null)
                            Spacer(Modifier.width(6.dp))
                            Text("Save CSV (${recordedSamples.size})")
                        }
                    }

                    val t = latestTelemetry
                    if (t == null) {
                        ElevatedCard(shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth()) {
                            Text(
                                "Tap Start Logging to stream live Boost, Oil Temp, Coolant Temp, RPM, and Voltage.",
                                modifier = Modifier.padding(16.dp),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    } else {
                        val metrics = listOf(
                            "Boost Pressure" to "${"%.1f".format(t.boostKpa)} kPa (${"%.1f".format((t.boostKpa - 101.3) * 0.145)} psi)",
                            "Oil Temperature" to "${"%.1f".format(t.oilTempC)} °C",
                            "Coolant Temp" to "${"%.1f".format(t.coolantTempC)} °C",
                            "Engine RPM" to "${t.rpm} rpm",
                            "Intake Air Temp" to "${"%.1f".format(t.intakeTempC)} °C",
                            "Battery Voltage" to "${"%.2f".format(t.batteryVoltage)} V",
                            "Vehicle Speed" to "${"%.0f".format(t.speedKmh)} km/h",
                        )
                        LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            items(metrics) { (label, value) ->
                                ElevatedCard(shape = RoundedCornerShape(14.dp), modifier = Modifier.fillMaxWidth()) {
                                    Row(
                                        modifier = Modifier.padding(14.dp).fillMaxWidth(),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                        Text(value, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
                                    }
                                }
                            }
                        }
                    }
                }
                3 -> {
                    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        Button(
                            enabled = !busy,
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1f),
                            onClick = {
                                if (demoMode) {
                                    dtcs = listOf(
                                        DtcUi("FEM_BODY", "8040B9", "Terminal 30B deactivation: startability limit reached", active = false),
                                        DtcUi("HU_NBT", "30FF", "Boost pressure below target (underboost)", active = false),
                                    )
                                    statusText = "Demo: loaded ${dtcs.size} fault code(s)."
                                    return@Button
                                }
                                runNative(
                                    "Reading fault codes",
                                    onSuccess = { data ->
                                        val arr = data as? JSONArray ?: JSONArray()
                                        dtcs = List(arr.length()) { i ->
                                            val obj = arr.getJSONObject(i)
                                            DtcUi(
                                                ecu = obj.optString("ecu"),
                                                code = obj.optString("code"),
                                                description = obj.optString("description"),
                                                active = obj.optBoolean("active"),
                                            )
                                        }
                                    },
                                ) { HandosBridge.nativeReadDtcs(host, backupDir) }
                            },
                        ) {
                            Icon(Icons.Rounded.Search, contentDescription = null, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Read Faults")
                        }
                        OutlinedButton(
                            enabled = !busy,
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1f),
                            onClick = {
                                if (demoMode) {
                                    dtcs = emptyList()
                                    statusText = "Demo: cleared all fault codes."
                                    return@OutlinedButton
                                }
                                runNative("Clearing fault codes", onSuccess = { dtcs = emptyList() }) {
                                    HandosBridge.nativeClearDtcs(host, backupDir)
                                }
                            },
                        ) {
                            Icon(Icons.Rounded.DeleteSweep, contentDescription = null, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Clear All")
                        }
                    }

                    if (dtcs.isEmpty()) {
                        ElevatedCard(shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth()) {
                            Text(
                                "No fault codes loaded.",
                                modifier = Modifier.padding(16.dp),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    } else {
                        LazyColumn(verticalArrangement = Arrangement.spacedBy(10.dp), contentPadding = PaddingValues(bottom = 12.dp)) {
                            items(dtcs) { dtc ->
                                ElevatedCard(shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth()) {
                                    Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                        val stateLabel = if (dtc.active) "ACTIVE" else "Historic"
                                        Text("${dtc.ecu} | ${dtc.code} ($stateLabel)", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                                        if (dtc.description.isNotBlank()) {
                                            Text(dtc.description, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                        }
                                        Spacer(Modifier.height(4.dp))
                                        OutlinedButton(
                                            shape = RoundedCornerShape(12.dp),
                                            onClick = {
                                                val query = Uri.encode("BMW ${dtc.ecu} ${dtc.code}")
                                                context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://www.google.com/search?q=$query")))
                                            },
                                        ) {
                                            Icon(Icons.Rounded.Search, contentDescription = null, modifier = Modifier.size(18.dp))
                                            Spacer(Modifier.width(6.dp))
                                            Text("Search Online")
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun CodingProgressDialog(
    state: CodingDialogState,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = { if (state.finished) onDismiss() },
        title = { Text("${state.title} (${state.ecu})", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                state.steps.forEach { step ->
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        when (step.state) {
                            StepState.Pending -> Icon(Icons.Rounded.RadioButtonUnchecked, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(20.dp))
                            StepState.Active -> CircularProgressIndicator(strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
                            StepState.Done -> Icon(Icons.Rounded.CheckCircle, contentDescription = null, tint = MaterialTheme.colorScheme.secondary, modifier = Modifier.size(20.dp))
                            StepState.Failed -> Icon(Icons.Rounded.ErrorOutline, contentDescription = null, tint = MaterialTheme.colorScheme.error, modifier = Modifier.size(20.dp))
                        }
                        Text(step.label, style = MaterialTheme.typography.bodySmall)
                    }
                }
                if (state.diffs.isNotEmpty()) {
                    HorizontalDivider()
                    Text("Applied Changes:", style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.SemiBold)
                    state.diffs.forEach { diff -> Text(diff, style = MaterialTheme.typography.bodySmall) }
                }
                state.backupPath?.let { path ->
                    Text("Backup: $path", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                state.error?.let { err ->
                    Text(err, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                }
            }
        },
        confirmButton = {
            Button(enabled = state.finished, onClick = onDismiss) { Text("Done") }
        },
    )
}

@Preview(showBackground = true)
@Composable
fun HandosAppPreview() {
    HandosTheme(darkTheme = true) {
        HandosApp(
            backupDir = "/tmp",
            logsDir = "/tmp",
            darkTheme = true,
            onToggleTheme = {},
            onBindWifi = { true },
        )
    }
}
