package com.handos.bmw

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities

/**
 * Ensures TCP traffic to the MHD Universal WiFi adapter (192.168.4.1:6801)
 * stays routed over WiFi even when Android detects no internet on the MHD AP.
 */
object MhdWifiBinder {
    fun bindProcessToWifi(context: Context): Boolean {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val wifiNetwork: Network? = cm.allNetworks.firstOrNull { network ->
            val caps = cm.getNetworkCapabilities(network)
            caps != null && caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
        }
        return if (wifiNetwork != null) {
            cm.bindProcessToNetwork(wifiNetwork)
        } else {
            false
        }
    }

    fun clearBinding(context: Context) {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        cm.bindProcessToNetwork(null)
    }
}
