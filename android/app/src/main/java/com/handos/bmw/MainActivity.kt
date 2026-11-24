package com.handos.bmw

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
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

data class PresetUi(
    val id: String,
    val family: String,
    val ecu: String,
    val title: String,
    val description: String,
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
        val schemasDir = filesDir.resolve("schemas").absolutePath
        setContent {
            MaterialTheme {
                HandosApp(
                    backupDir = backupDir,
                    schemasDir = schemasDir,
                    onBindWifi = { MhdWifiBinder.bindProcessToWifi(this) }
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HandosApp(backupDir: String, schemasDir: String, onBindWifi: () -> Boolean) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var selectedTab by remember { mutableIntStateOf(0) }
    var host by remember { mutableStateOf(HandosBridge.DEFAULT_MHD_HOST) }
    var statusText by remember { mutableStateOf("Ready. Connect phone to MHD WiFi AP.") }
    var busy by remember { mutableStateOf(false) }
    var dtcs by remember { mutableStateOf<List<DtcUi>>(emptyList()) }

    val schemaPicker = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenMultipleDocuments()
    ) { uris ->
        if (uris.isEmpty()) return@rememberLauncherForActivityResult
        scope.launch {
            var imported = 0
            withContext(Dispatchers.IO) {
                for (uri in uris) {
                    val name = context.contentResolver.query(uri, null, null, null, null)?.use { c ->
                        val idx = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        if (c.moveToFirst() && idx >= 0) c.getString(idx) else "schema.json"
                    } ?: "schema.json"
                    val content = context.contentResolver.openInputStream(uri)?.bufferedReader()?.readText()
                    if (content != null && HandosBridge.isNativeLoaded) {
                        HandosBridge.nativeImportSchema(schemasDir, name, content)
                        imported++
                    }
                }
            }
            statusText = "Imported $imported schema file(s) into $schemasDir."
        }
    }

    val presets = remember {
        listOf(
            PresetUi("e46_usdm_corners", "E46", "LSZ", "USDM Corner Lights (E46)", "Front turn signals as sidemarkers"),
            PresetUi("e46_double_hazard", "E46", "LSZ", "Double Hazard Lights (E46)", "Double-impulse hazard flash"),
            PresetUi("e46_comfort_windows", "E46", "GM5", "Comfort Windows (E46)", "Hold fob to open/close windows"),
            PresetUi("e8x_e9x_usdm_corners", "E8x/E9x", "FRM", "USDM Corner Lights (1/3-Series)", "Dimmed front turn signals as sidemarkers"),
            PresetUi("e8x_e9x_double_hazard", "E8x/E9x", "FRM", "Double Hazard Lights (1/3-Series)", "Double-impulse hazard blink"),
            PresetUi("e8x_e9x_start_stop_memory", "E8x/E9x", "IHKA", "Auto Start/Stop Memory (1/3-Series)", "Remember last Start/Stop state"),
            PresetUi("e8x_e9x_digital_speedo", "E8x/E9x", "KOMBI", "Digital Speedometer (1/3-Series)", "Uncorrected digital speed in cluster"),
        )
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
                listOf("Connect", "Presets", "Faults").forEachIndexed { idx, label ->
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
                            runNative("Scanning vehicle") {
                                HandosBridge.nativeScanVehicle(host, backupDir)
                            }
                        },
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("Bind WiFi & Scan Vehicle")
                    }
                    OutlinedButton(
                        enabled = !busy,
                        onClick = { schemaPicker.launch(arrayOf("*/*")) },
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("Import ECU Schemas (.json / .xml)")
                    }
                }
                1 -> {
                    LazyColumn(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        items(presets) { preset ->
                            Card(modifier = Modifier.fillMaxWidth()) {
                                Column(modifier = Modifier.padding(12.dp)) {
                                    Text(
                                        "${preset.title} [${preset.family} • ${preset.ecu}]",
                                        style = MaterialTheme.typography.titleMedium
                                    )
                                    Text(preset.description, style = MaterialTheme.typography.bodySmall)
                                    Spacer(Modifier.height(8.dp))
                                    Button(
                                        enabled = !busy,
                                        onClick = {
                                            runNative("Applying ${preset.title}") {
                                                HandosBridge.nativeApplyPreset(host, preset.id, backupDir)
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
