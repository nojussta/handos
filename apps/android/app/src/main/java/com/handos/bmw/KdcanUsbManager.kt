package com.handos.bmw

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbManager
import android.os.Build

data class KdcanProbeResult(
    val detected: Boolean,
    val hasPermission: Boolean,
    val message: String,
    val deviceName: String? = null,
)

/**
 * Detects and configures FTDI-based BMW K+DCAN USB cables over USB-C OTG.
 * Common FTDI VID: 0x0403, PIDs: 0x6001 (FT232RL), 0x6015 (FT231X).
 */
object KdcanUsbManager {
    private const val ACTION_USB_PERMISSION = "com.handos.bmw.USB_PERMISSION"
    private const val FTDI_VID = 0x0403
    private val SUPPORTED_PIDS = setOf(0x6001, 0x6010, 0x6015)

    fun findKdcanDevice(context: Context): UsbDevice? {
        val usbManager = context.getSystemService(Context.USB_SERVICE) as? UsbManager ?: return null
        return usbManager.deviceList.values.firstOrNull { dev ->
            dev.vendorId == FTDI_VID && dev.productId in SUPPORTED_PIDS
        }
    }

    fun probeOrRequestPermission(context: Context): KdcanProbeResult {
        val usbManager = context.getSystemService(Context.USB_SERVICE) as? UsbManager
            ?: return KdcanProbeResult(false, false, "USB Host service unavailable on this device.")

        val device = findKdcanDevice(context)
            ?: return KdcanProbeResult(
                detected = false,
                hasPermission = false,
                message = "No FTDI K+DCAN USB cable detected. Check USB-C OTG adapter.",
            )

        val devLabel = "${device.productName ?: "FTDI K+DCAN"} (0x${device.vendorId.toString(16)}:0x${device.productId.toString(16)})"
        if (!usbManager.hasPermission(device)) {
            val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                PendingIntent.FLAG_MUTABLE
            } else {
                0
            }
            val pi = PendingIntent.getBroadcast(context, 0, Intent(ACTION_USB_PERMISSION), flags)
            usbManager.requestPermission(device, pi)
            return KdcanProbeResult(
                detected = true,
                hasPermission = false,
                message = "Requested USB permission for $devLabel. Tap Allow, then Scan again.",
                deviceName = device.deviceName,
            )
        }

        return KdcanProbeResult(
            detected = true,
            hasPermission = true,
            message = "K+DCAN USB ready: $devLabel",
            deviceName = device.deviceName,
        )
    }

    /**
     * Opens the FTDI device and configures baud rate (115200 for D-CAN or 10400 for K-Line).
     */
    fun openConfiguredConnection(context: Context, baudRate: Int = 115_200): UsbDeviceConnection? {
        val usbManager = context.getSystemService(Context.USB_SERVICE) as? UsbManager ?: return null
        val device = findKdcanDevice(context) ?: return null
        if (!usbManager.hasPermission(device)) return null
        val conn = usbManager.openDevice(device) ?: return null

        // FTDI reset (0x00), baud rate divisor (0x03), 8N1 line coding (0x04)
        conn.controlTransfer(0x40, 0x00, 0x0000, 0, null, 0, 1000)
        val divisor = (3_000_000 / baudRate).coerceAtLeast(1)
        conn.controlTransfer(0x40, 0x03, divisor, 0, null, 0, 1000)
        conn.controlTransfer(0x40, 0x04, 0x0008, 0, null, 0, 1000)
        return conn
    }
}
