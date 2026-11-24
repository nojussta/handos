package com.handos.bmw

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

data class CodingItemUi(
    val id: String,
    val family: String,
    val ecu: String,
    val title: String,
    val description: String,
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

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val backupDir = filesDir.resolve("backups").absolutePath
        setContent {
            MaterialTheme {
                HandosApp(
                    backupDir = backupDir,
                    onBindWifi = { MhdWifiBinder.bindProcessToWifi(this) }
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
fun HandosApp(backupDir: String, onBindWifi: () -> Boolean) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var selectedTab by remember { mutableIntStateOf(0) }
    var host by remember { mutableStateOf(HandosBridge.DEFAULT_MHD_HOST) }
    var statusText by remember { mutableStateOf("Ready. Connect phone to MHD WiFi AP.") }
    var busy by remember { mutableStateOf(false) }
    var scannedVehicle by remember { mutableStateOf<VehicleUi?>(null) }
    var showAllVehicles by remember { mutableStateOf(false) }
    var dtcs by remember { mutableStateOf<List<DtcUi>>(emptyList()) }

    val codingItems = remember {
        listOf(
            // E46
            CodingItemUi("e46_usdm_corners", "E46", "LSZ", "USDM Corner Lights (E46)", "Front turn signals as sidemarkers"),
            CodingItemUi("e46_double_hazard", "E46", "LSZ", "Double Hazard Lights (E46)", "Double-impulse hazard flash"),
            CodingItemUi("e46_comfort_windows", "E46", "GM5", "Comfort Windows (E46)", "Hold fob to open/close windows"),
            // E8x / E9x (1-Series & 3-Series)
            CodingItemUi("e8x_e9x_usdm_corners", "E8xE9x", "FRM", "USDM Corner Lights (E8x / E9x)", "Dimmed front turn signals as sidemarkers"),
            CodingItemUi("e8x_e9x_double_hazard", "E8xE9x", "FRM", "Double Hazard Lights (E8x / E9x)", "Double-impulse hazard blink"),
            CodingItemUi("e8x_e9x_start_stop_memory", "E8xE9x", "IHKA", "Auto Start/Stop Memory (E8x / E9x)", "Remember last Start/Stop state"),
            CodingItemUi("e8x_e9x_digital_speedo", "E8xE9x", "KOMBI", "Digital Speedometer (E8x / E9x)", "Uncorrected digital speed in cluster"),
            // F-Series (F10 / F20 / F30 / F8x)
            CodingItemUi("fxx_start_stop_memory", "Fxx", "FEM_BODY", "Auto Start/Stop Memory (F-Series)", "Remember last Start/Stop state across restarts"),
            CodingItemUi("fxx_start_stop_off_default", "Fxx", "FEM_BODY", "Auto Start/Stop Off by Default (F-Series)", "Default Start/Stop to OFF on every start"),
            CodingItemUi("fxx_digital_speedo", "Fxx", "KOMBI", "Digital Speedometer (F-Series)", "Show digital speed readout in cluster"),
            CodingItemUi("fxx_mirrors_fold_lock", "Fxx", "FEM_BODY", "Fold Mirrors on Lock (F-Series)", "Fold side mirrors on key fob / comfort lock"),
            CodingItemUi("fxx_usdm_sidemarkers", "Fxx", "FEM_BODY", "USDM Corner Sidemarkers (F-Series)", "Enable US-spec front amber sidemarkers"),
            CodingItemUi("fxx_video_in_motion", "Fxx", "HU_NBT", "Video in Motion (F-Series iDrive)", "Remove iDrive video speed lock"),
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

    fun runNative(
        actionName: String,
        onSuccess: ((Any?) -> Unit)? = null,
        block: () -> String,
    ) {
        if (!HandosBridge.isNativeLoaded) {
            statusText = "$actionName: libhandos_core.so not bundled yet (build with cargo-ndk)."
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

    Scaffold(
        topBar = { TopAppBar(title = { Text("Handos BMW Coding") }) },
        bottomBar = {
            NavigationBar {
                listOf("Connect", "Coding", "Faults").forEachIndexed { idx, label ->
                    NavigationBarItem(
                        selected = selectedTab == idx,
                        onClick = { selectedTab = idx },
                        label = { Text(label) },
                        icon = {}
                    )
                }
            }
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .padding(padding)
                .padding(16.dp)
                .fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            Card(modifier = Modifier.fillMaxWidth()) {
                Text(
                    text = statusText,
                    modifier = Modifier.padding(12.dp),
                    style = MaterialTheme.typography.bodyMedium
                )
            }

            if (busy) {
                LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
            }

            when (selectedTab) {
                0 -> {
                    OutlinedTextField(
                        value = host,
                        onValueChange = { host = it },
                        label = { Text("MHD WiFi Adapter Endpoint") },
                        modifier = Modifier.fillMaxWidth()
                    )
                    Button(
                        enabled = !busy,
                        onClick = {
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
                                    scannedVehicle = VehicleUi(
                                        vin = obj.optString("vin"),
                                        chassis = chassis,
                                        platformFamily = inferPlatformFamily(chassis),
                                        batteryVoltage = obj.optDouble("battery_voltage", 0.0),
                                        ecus = ecus,
                                    )
                                }
                            ) {
                                HandosBridge.nativeScanVehicle(host, backupDir)
                            }
                        },
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("Bind WiFi & Scan Vehicle")
                    }

                    scannedVehicle?.let { v ->
                        Card(modifier = Modifier.fillMaxWidth()) {
                            Column(modifier = Modifier.padding(12.dp)) {
                                Text("Connected: ${v.chassis} (${v.vin})", style = MaterialTheme.typography.titleMedium)
                                Text("Battery: ${"%.2f".format(v.batteryVoltage)}V | ECUs: ${v.ecus.joinToString(", ")}")
                            }
                        }
                    }
                }
                1 -> {
                    scannedVehicle?.let { v ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween
                        ) {
                            Text(
                                if (showAllVehicles) "Showing all models" else "Filtered for ${v.chassis}",
                                style = MaterialTheme.typography.labelLarge
                            )
                            TextButton(onClick = { showAllVehicles = !showAllVehicles }) {
                                Text(if (showAllVehicles) "Filter to Car" else "Show All")
                            }
                        }
                    }

                    LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        items(visibleCodingItems) { item ->
                            Card(modifier = Modifier.fillMaxWidth()) {
                                Column(modifier = Modifier.padding(12.dp)) {
                                    Text(
                                        "${item.title} [${item.family} | ${item.ecu}]",
                                        style = MaterialTheme.typography.titleMedium
                                    )
                                    Text(item.description, style = MaterialTheme.typography.bodySmall)
                                    Spacer(Modifier.height(8.dp))
                                    Button(
                                        enabled = !busy,
                                        onClick = {
                                            runNative("Applying ${item.title}") {
                                                HandosBridge.nativeApplyPreset(host, item.id, backupDir)
                                            }
                                        }
                                    ) {
                                        Text("Backup & Code Safely")
                                    }
                                }
                            }
                        }
                    }
                }
                2 -> {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Button(
                            enabled = !busy,
                            onClick = {
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
                                    }
                                ) {
                                    HandosBridge.nativeReadDtcs(host, backupDir)
                                }
                            }
                        ) {
                            Text("Read Faults")
                        }
                        OutlinedButton(
                            enabled = !busy,
                            onClick = {
                                runNative(
                                    "Clearing fault codes",
                                    onSuccess = { dtcs = emptyList() }
                                ) {
                                    HandosBridge.nativeClearDtcs(host, backupDir)
                                }
                            }
                        ) {
                            Text("Clear All Faults")
                        }
                    }

                    LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        items(dtcs) { dtc ->
                            Card(modifier = Modifier.fillMaxWidth()) {
                                Column(modifier = Modifier.padding(12.dp)) {
                                    val stateLabel = if (dtc.active) "ACTIVE" else "Historic"
                                    Text(
                                        "${dtc.ecu} | ${dtc.code} ($stateLabel)",
                                        style = MaterialTheme.typography.titleMedium
                                    )
                                    if (dtc.description.isNotBlank()) {
                                        Text(dtc.description, style = MaterialTheme.typography.bodySmall)
                                    }
                                    Spacer(Modifier.height(8.dp))
                                    OutlinedButton(onClick = {
                                        val query = Uri.encode("BMW ${dtc.ecu} ${dtc.code}")
                                        val intent = Intent(
                                            Intent.ACTION_VIEW,
                                            Uri.parse("https://www.google.com/search?q=$query")
                                        )
                                        context.startActivity(intent)
                                    }) {
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
