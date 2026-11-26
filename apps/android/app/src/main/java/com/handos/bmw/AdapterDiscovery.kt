package com.handos.bmw

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.withContext

enum class AdapterKind {
    MHD_WIFI,
    KDCAN_USB,
}

data class DetectedAdapter(
    val id: String,
    val kind: AdapterKind,
    val name: String,
    val detail: String,
    val endpoint: String,
    val isReady: Boolean,
    val needsPermission: Boolean = false,
)

/**
 * Automatically probes for reachable MHD Universal WiFi adapters and attached FTDI K+DCAN USB cables.
 */
object AdapterDiscovery {
    private val DEFAULT_MHD_ENDPOINTS = listOf(
        HandosBridge.DEFAULT_MHD_HOST,
        "192.168.1.1:6801",
    )

    suspend fun detectAll(
        context: Context,
        extraMhdCandidates: List<String> = emptyList(),
    ): List<DetectedAdapter> = withContext(Dispatchers.IO) {
        val wifiNetwork = MhdWifiBinder.findWifiNetwork(context)
        val gatewayHost = MhdWifiBinder.getWifiGatewayHost(context)

        val candidates = buildList {
            if (!gatewayHost.isNullOrBlank()) {
                add("$gatewayHost:6801")
            }
            addAll(DEFAULT_MHD_ENDPOINTS)
            for (extra in extraMhdCandidates) {
                val trimmed = extra.trim()
                if (trimmed.isNotEmpty()) add(trimmed)
            }
        }.distinct()

        val mhdAdapters = coroutineScope {
            candidates.map { candidate ->
                async {
                    val reachable = MhdWifiBinder.probeTcpEndpoint(wifiNetwork, candidate, timeoutMs = 700)
                    if (reachable) {
                        DetectedAdapter(
                            id = "mhd:$candidate",
                            kind = AdapterKind.MHD_WIFI,
                            name = "MHD Universal WiFi",
                            detail = "$candidate (WiFi reachable)",
                            endpoint = candidate,
                            isReady = true,
                            needsPermission = false,
                        )
                    } else {
                        null
                    }
                }
            }.awaitAll().filterNotNull()
        }

        val kdcanAdapters = KdcanUsbManager.detectDevices(context).map { dev ->
            DetectedAdapter(
                id = "kdcan:${dev.deviceName}",
                kind = AdapterKind.KDCAN_USB,
                name = "K+DCAN USB (FTDI)",
                detail = if (dev.hasPermission) {
                    "${dev.label} (Ready)"
                } else {
                    "${dev.label} (Tap to grant USB permission)"
                },
                endpoint = dev.deviceName,
                isReady = dev.hasPermission,
                needsPermission = !dev.hasPermission,
            )
        }

        mhdAdapters + kdcanAdapters
    }
}
