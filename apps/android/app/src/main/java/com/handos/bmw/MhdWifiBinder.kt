package com.handos.bmw

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import java.net.Inet4Address
import java.net.InetSocketAddress
import java.net.Socket

/**
 * Ensures TCP traffic to the MHD Universal WiFi adapter (192.168.4.1:6801)
 * stays routed over WiFi even when Android detects no internet on the MHD AP.
 */
object MhdWifiBinder {
    fun findWifiNetwork(context: Context): Network? {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return null
        return cm.allNetworks.firstOrNull { network ->
            val caps = cm.getNetworkCapabilities(network)
            caps != null && caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
        }
    }

    fun getWifiGatewayHost(context: Context): String? {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return null
        val wifiNetwork = findWifiNetwork(context) ?: return null
        val linkProps = cm.getLinkProperties(wifiNetwork) ?: return null
        val gateway = linkProps.routes
            .firstOrNull { it.isDefaultRoute && it.gateway is Inet4Address }
            ?.gateway
            ?: linkProps.routes.firstOrNull { it.gateway is Inet4Address }?.gateway
        return gateway?.hostAddress
    }

    fun probeTcpEndpoint(network: Network?, hostPort: String, timeoutMs: Int = 700): Boolean {
        val idx = hostPort.lastIndexOf(':')
        if (idx <= 0 || idx >= hostPort.length - 1) return false
        val host = hostPort.substring(0, idx).trim()
        val port = hostPort.substring(idx + 1).trim().toIntOrNull() ?: return false
        return try {
            val socket = network?.socketFactory?.createSocket() ?: Socket()
            socket.use { s ->
                s.connect(InetSocketAddress(host, port), timeoutMs)
                true
            }
        } catch (_: Exception) {
            false
        }
    }

    fun bindProcessToWifi(context: Context): Boolean {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return false
        val wifiNetwork = findWifiNetwork(context)
        return if (wifiNetwork != null) {
            cm.bindProcessToNetwork(wifiNetwork)
        } else {
            false
        }
    }

    fun clearBinding(context: Context) {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return
        cm.bindProcessToNetwork(null)
    }
}
