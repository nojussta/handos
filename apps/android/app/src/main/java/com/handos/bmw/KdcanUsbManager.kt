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

data class KdcanDetectedDevice(
    val deviceName: String,
    val label: String,
    val hasPermission: Boolean,
)

/**
 * Detects and configures FTDI-based BMW K+DCAN USB cables over USB-C OTG.
 * Common FTDI VID: 0x0403, PIDs: 0x6001 (FT232RL), 0x6010 (FT2232), 0x6015 (FT231X).
 */
object KdcanUsbManager {
    const val ACTION_USB_PERMISSION = "com.handos.bmw.USB_PERMISSION"
    private const val FTDI_VID = 0x0403
    private val SUPPORTED_PIDS = setOf(0x6001, 0x6010, 0x6015)

    fun findAllKdcanDevices(context: Context): List<UsbDevice> {
        val usbManager = context.getSystemService(Context.USB_SERVICE) as? UsbManager ?: return emptyList()
        return usbManager.deviceList.values.filter { dev ->
            dev.vendorId == FTDI_VID && dev.productId in SUPPORTED_PIDS
        }
    }

    fun findKdcanDevice(context: Context, deviceName: String? = null): UsbDevice? {
        val devices = findAllKdcanDevices(context)
        return if (deviceName != null) {
            devices.firstOrNull { it.deviceName == deviceName } ?: devices.firstOrNull()
        } else {
            devices.firstOrNull()
        }
    }

    private fun deviceLabel(device: UsbDevice): String {
        return "${device.productName ?: "FTDI K+DCAN"} (0x${device.vendorId.toString(16)}:0x${device.productId.toString(16)})"
    }

    fun detectDevices(context: Context): List<KdcanDetectedDevice> {
        val usbManager = context.getSystemService(Context.USB_SERVICE) as? UsbManager ?: return emptyList()
        return findAllKdcanDevices(context).map { dev ->
            KdcanDetectedDevice(
                deviceName = dev.deviceName,
                label = deviceLabel(dev),
                hasPermission = usbManager.hasPermission(dev),
            )
        }
    }

    fun probeOrRequestPermission(context: Context, deviceName: String? = null): KdcanProbeResult {
        val usbManager = context.getSystemService(Context.USB_SERVICE) as? UsbManager
            ?: return KdcanProbeResult(false, false, "USB Host service unavailable on this device.")

        val device = findKdcanDevice(context, deviceName)
            ?: return KdcanProbeResult(
                detected = false,
                hasPermission = false,
                message = "No FTDI K+DCAN USB cable detected. Check USB-C OTG adapter.",
            )

        val devLabel = deviceLabel(device)
        if (!usbManager.hasPermission(device)) {
            val flags = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            } else {
                PendingIntent.FLAG_UPDATE_CURRENT
            }
            val permIntent = Intent(ACTION_USB_PERMISSION).apply {
                setPackage(context.packageName)
            }
            val pi = PendingIntent.getBroadcast(context, 0, permIntent, flags)
            usbManager.requestPermission(device, pi)
            return KdcanProbeResult(
                detected = true,
                hasPermission = false,
                message = "Requested USB permission for $devLabel. Tap Allow, then try again.",
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
    fun openConfiguredConnection(
        context: Context,
        baudRate: Int = 115_200,
        deviceName: String? = null,
    ): UsbDeviceConnection? {
        val usbManager = context.getSystemService(Context.USB_SERVICE) as? UsbManager ?: return null
        val device = findKdcanDevice(context, deviceName) ?: return null
        if (!usbManager.hasPermission(device)) return null
        val conn = usbManager.openDevice(device) ?: return null

        // FTDI reset (0x00), baud rate divisor (0x03), 8N1 line coding (0x04)
        conn.controlTransfer(0x40, 0x00, 0x0000, 0, null, 0, 1000)
        val divisor = (3_000_000 / baudRate).coerceAtLeast(1)
        conn.controlTransfer(0x40, 0x03, divisor, 0, null, 0, 1000)
        conn.controlTransfer(0x40, 0x04, 0x0008, 0, null, 0, 1000)
        return conn
    }

    /**
     * Opens the K+DCAN USB device for the duration of [block], passing a `kdcan-fd://<fd>?baud=<rate>`
     * endpoint descriptor to Rust FFI, and closes the USB connection cleanly afterward.
     */
    fun withKdcanEndpoint(
        context: Context,
        deviceName: String? = null,
        baudRate: Int = 115_200,
        block: (String) -> String,
    ): String {
        val conn = openConfiguredConnection(context, baudRate, deviceName)
            ?: return """{"ok":false,"error":"K+DCAN USB cable not ready or permission denied."}"""
        return try {
            val fd = conn.fileDescriptor
            if (fd < 0) {
                """{"ok":false,"error":"Invalid USB file descriptor."}"""
            } else {
                block("kdcan-fd://$fd?baud=$baudRate")
            }
        } finally {
            runCatching { conn.close() }
        }
    }
}
