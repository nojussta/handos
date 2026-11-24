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
import androidx.compose.material.icons.rounded.Build
import androidx.compose.material.icons.rounded.DarkMode
import androidx.compose.material.icons.rounded.DeleteSweep
import androidx.compose.material.icons.rounded.DirectionsCar
import androidx.compose.material.icons.rounded.LightMode
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Tune
import androidx.compose.material.icons.rounded.WarningAmber
import androidx.compose.material.icons.rounded.Wifi
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
        enableEdgeToEdge()
        val backupDir = filesDir.resolve("backups").absolutePath
        setContent {
            val systemDark = isSystemInDarkTheme()
            var darkTheme by remember { mutableStateOf(systemDark) }
            HandosTheme(darkTheme = darkTheme) {
                HandosApp(
                    backupDir = backupDir,
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
    darkTheme: Boolean,
    onToggleTheme: () -> Unit,
    onBindWifi: () -> Boolean,
) {
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
            CodingItemUi("e46_usdm_corners", "E46", "LSZ", "USDM Corner Lights", "Front turn signals as sidemarkers"),
            CodingItemUi("e46_double_hazard", "E46", "LSZ", "Double Hazard Lights", "Double-impulse hazard flash"),
            CodingItemUi("e46_comfort_windows", "E46", "GM5", "Comfort Windows", "Hold fob to open/close windows"),
            CodingItemUi("e8x_e9x_usdm_corners", "E8xE9x", "FRM", "USDM Corner Lights", "Dimmed front turn signals as sidemarkers"),
            CodingItemUi("e8x_e9x_double_hazard", "E8xE9x", "FRM", "Double Hazard Lights", "Double-impulse hazard blink"),
            CodingItemUi("e8x_e9x_start_stop_memory", "E8xE9x", "IHKA", "Auto Start/Stop Memory", "Remember last Start/Stop state"),
            CodingItemUi("e8x_e9x_digital_speedo", "E8xE9x", "KOMBI", "Digital Speedometer", "Uncorrected digital speed in cluster"),
            CodingItemUi("fxx_start_stop_memory", "Fxx", "FEM_BODY", "Auto Start/Stop Memory", "Remember last Start/Stop state across restarts"),
            CodingItemUi("fxx_start_stop_off_default", "Fxx", "FEM_BODY", "Auto Start/Stop Off by Default", "Default Start/Stop to OFF on every start"),
            CodingItemUi("fxx_digital_speedo", "Fxx", "KOMBI", "Digital Speedometer", "Show digital speed readout in cluster"),
            CodingItemUi("fxx_mirrors_fold_lock", "Fxx", "FEM_BODY", "Fold Mirrors on Lock", "Fold side mirrors on key fob / comfort lock"),
            CodingItemUi("fxx_usdm_sidemarkers", "Fxx", "FEM_BODY", "USDM Corner Sidemarkers", "Enable US-spec front amber sidemarkers"),
            CodingItemUi("fxx_video_in_motion", "Fxx", "HU_NBT", "Video in Motion", "Remove iDrive video speed lock"),
            CodingItemUi("fxx_nbt_evo_aa_prep", "Fxx", "HU_NBT", "NBT EVO Android Auto Prep", "Enable CarLife slot & WLAN flags for NBT EVO AA"),
            CodingItemUi("fxx_nbt_evo_carplay_fullscreen", "Fxx", "HU_NBT", "NBT EVO CarPlay Fullscreen", "Enable fullscreen CarPlay on NBT EVO ID5/ID6"),
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
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = "handos",
                        fontWeight = FontWeight.Bold,
                    )
                },
                actions = {
                    IconButton(onClick = onToggleTheme) {
                        Icon(
                            imageVector = if (darkTheme) Icons.Rounded.LightMode else Icons.Rounded.DarkMode,
                            contentDescription = "Toggle theme",
                        )
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = MaterialTheme.colorScheme.background,
                ),
            )
        },
        bottomBar = {
            NavigationBar(containerColor = MaterialTheme.colorScheme.surface) {
                val tabs = listOf(
                    "Connect" to Icons.Rounded.Wifi,
                    "Coding" to Icons.Rounded.Tune,
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
                    ElevatedCard(
                        shape = RoundedCornerShape(18.dp),
                        colors = CardDefaults.elevatedCardColors(
                            containerColor = MaterialTheme.colorScheme.surface,
                        ),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Column(
                            modifier = Modifier.padding(16.dp),
                            verticalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            Text(
                                "Adapter Connection",
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.SemiBold,
                            )
                            OutlinedTextField(
                                value = host,
                                onValueChange = { host = it },
                                label = { Text("MHD WiFi Endpoint") },
                                singleLine = true,
                                shape = RoundedCornerShape(12.dp),
                                modifier = Modifier.fillMaxWidth(),
                            )
                            Button(
                                enabled = !busy,
                                shape = RoundedCornerShape(14.dp),
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .height(50.dp),
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
                                        },
                                    ) {
                                        HandosBridge.nativeScanVehicle(host, backupDir)
                                    }
                                },
                            ) {
                                Icon(Icons.Rounded.DirectionsCar, contentDescription = null)
                                Spacer(Modifier.width(8.dp))
                                Text("Connect & Scan Vehicle")
                            }
                        }
                    }

                    scannedVehicle?.let { v ->
                        ElevatedCard(
                            shape = RoundedCornerShape(18.dp),
                            colors = CardDefaults.elevatedCardColors(
                                containerColor = MaterialTheme.colorScheme.surface,
                            ),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Column(
                                modifier = Modifier.padding(16.dp),
                                verticalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Row(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Text(
                                        v.chassis,
                                        style = MaterialTheme.typography.titleMedium,
                                        fontWeight = FontWeight.Bold,
                                    )
                                    AssistChip(
                                        onClick = {},
                                        label = { Text("${"%.2f".format(v.batteryVoltage)} V") },
                                    )
                                }
                                Text(
                                    "VIN: ${v.vin}",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                LazyRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                    items(v.ecus.toList()) { ecuName ->
                                        AssistChip(onClick = {}, label = { Text(ecuName) })
                                    }
                                }
                            }
                        }
                    }
                }
                1 -> {
                    scannedVehicle?.let { v ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                if (showAllVehicles) "All models" else "Filtered for ${v.chassis}",
                                style = MaterialTheme.typography.labelLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            FilterChip(
                                selected = !showAllVehicles,
                                onClick = { showAllVehicles = !showAllVehicles },
                                label = { Text(if (showAllVehicles) "Filter to Car" else "Compatible Only") },
                            )
                        }
                    }

                    LazyColumn(
                        verticalArrangement = Arrangement.spacedBy(10.dp),
                        contentPadding = PaddingValues(bottom = 12.dp),
                    ) {
                        items(visibleCodingItems) { item ->
                            ElevatedCard(
                                shape = RoundedCornerShape(16.dp),
                                colors = CardDefaults.elevatedCardColors(
                                    containerColor = MaterialTheme.colorScheme.surface,
                                ),
                                modifier = Modifier.fillMaxWidth(),
                            ) {
                                Column(
                                    modifier = Modifier.padding(14.dp),
                                    verticalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
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
                                            label = { Text("${item.family} | ${item.ecu}") },
                                        )
                                    }
                                    Text(
                                        item.description,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                    Spacer(Modifier.height(4.dp))
                                    FilledTonalButton(
                                        enabled = !busy,
                                        shape = RoundedCornerShape(12.dp),
                                        onClick = {
                                            runNative("Applying ${item.title}") {
                                                HandosBridge.nativeApplyPreset(host, item.id, backupDir)
                                            }
                                        },
                                    ) {
                                        Icon(Icons.Rounded.Build, contentDescription = null, modifier = Modifier.size(18.dp))
                                        Spacer(Modifier.width(6.dp))
                                        Text("Backup & Code")
                                    }
                                }
                            }
                        }
                    }
                }
                2 -> {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        Button(
                            enabled = !busy,
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.weight(1f),
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
                                    },
                                ) {
                                    HandosBridge.nativeReadDtcs(host, backupDir)
                                }
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
                                runNative(
                                    "Clearing fault codes",
                                    onSuccess = { dtcs = emptyList() },
                                ) {
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
                        ElevatedCard(
                            shape = RoundedCornerShape(16.dp),
                            colors = CardDefaults.elevatedCardColors(
                                containerColor = MaterialTheme.colorScheme.surface,
                            ),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text(
                                "No fault codes loaded.",
                                modifier = Modifier.padding(16.dp),
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    } else {
                        LazyColumn(
                            verticalArrangement = Arrangement.spacedBy(10.dp),
                            contentPadding = PaddingValues(bottom = 12.dp),
                        ) {
                            items(dtcs) { dtc ->
                                ElevatedCard(
                                    shape = RoundedCornerShape(16.dp),
                                    colors = CardDefaults.elevatedCardColors(
                                        containerColor = MaterialTheme.colorScheme.surface,
                                    ),
                                    modifier = Modifier.fillMaxWidth(),
                                ) {
                                    Column(
                                        modifier = Modifier.padding(14.dp),
                                        verticalArrangement = Arrangement.spacedBy(6.dp),
                                    ) {
                                        val stateLabel = if (dtc.active) "ACTIVE" else "Historic"
                                        Text(
                                            "${dtc.ecu} | ${dtc.code} ($stateLabel)",
                                            style = MaterialTheme.typography.titleMedium,
                                            fontWeight = FontWeight.SemiBold,
                                        )
                                        if (dtc.description.isNotBlank()) {
                                            Text(
                                                dtc.description,
                                                style = MaterialTheme.typography.bodySmall,
                                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            )
                                        }
                                        Spacer(Modifier.height(4.dp))
                                        OutlinedButton(
                                            shape = RoundedCornerShape(12.dp),
                                            onClick = {
                                                val query = Uri.encode("BMW ${dtc.ecu} ${dtc.code}")
                                                val intent = Intent(
                                                    Intent.ACTION_VIEW,
                                                    Uri.parse("https://www.google.com/search?q=$query"),
                                                )
                                                context.startActivity(intent)
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

@Preview(showBackground = true)
@Composable
fun HandosAppPreview() {
    HandosTheme(darkTheme = true) {
        HandosApp(
            backupDir = "/tmp",
            darkTheme = true,
            onToggleTheme = {},
            onBindWifi = { true },
        )
    }
}
