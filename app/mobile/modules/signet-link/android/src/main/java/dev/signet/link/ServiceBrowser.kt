package dev.signet.link

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.net.Inet6Address
import java.net.InetAddress
import java.util.concurrent.Executor
import java.util.concurrent.Executors

/** A computer seen over mDNS, resolved. */
data class FoundService(val name: String, val desktopId: String, val version: Int, val hosts: List<String>, val port: Int)

/**
 * mDNS discovery of `_signet._tcp`. A multicast lock is held during discovery (otherwise many phones
 * filter mDNS packets). API 34+: one `ServiceInfoCallback` per service; before: `resolveService`, one at a time.
 */
class ServiceBrowser(
  context: Context,
  private val onFound: (FoundService) -> Unit,
  private val onLost: (String) -> Unit,
) {
  private val nsd = context.getSystemService(Context.NSD_SERVICE) as NsdManager
  private val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
  private val lock = Any()
  private val handler = Handler(Looper.getMainLooper())
  private val callbackExecutor: Executor = Executors.newSingleThreadExecutor()

  private var browsing = false
  private var multicastLock: WifiManager.MulticastLock? = null
  private var discovery: NsdManager.DiscoveryListener? = null

  /** Services already resolved, by instance name (returned on each `start`). */
  private val known = LinkedHashMap<String, FoundService>()

  // API 34+
  private val infoCallbacks = HashMap<String, NsdManager.ServiceInfoCallback>()

  // Before API 34
  private val pending = java.util.ArrayDeque<NsdServiceInfo>()
  private var resolving = false
  private var resolveSeq = 0

  fun start() {
    synchronized(lock) {
      if (browsing) {
        known.values.toList().forEach(onFound)
        return
      }
      try {
        multicastLock = wifi.createMulticastLock("signetlink").apply {
          setReferenceCounted(false)
          acquire()
        }
      } catch (_: Exception) {
        multicastLock = null // discovery may work without it (depending on the phone)
      }
      val listener = makeDiscoveryListener()
      try {
        nsd.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
        discovery = listener
        browsing = true
      } catch (_: Exception) {
        releaseMulticast()
      }
    }
  }

  fun stop() {
    synchronized(lock) {
      if (!browsing) return
      browsing = false
      discovery?.let {
        try {
          nsd.stopServiceDiscovery(it)
        } catch (_: Exception) {
        }
      }
      discovery = null
      if (Build.VERSION.SDK_INT >= 34) {
        infoCallbacks.values.forEach { unregister(it) }
        infoCallbacks.clear()
      }
      pending.clear()
      resolving = false
      resolveSeq++
      known.clear()
      releaseMulticast()
    }
  }

  private fun releaseMulticast() {
    try {
      multicastLock?.takeIf { it.isHeld }?.release()
    } catch (_: Exception) {
    }
    multicastLock = null
  }

  private fun makeDiscoveryListener() = object : NsdManager.DiscoveryListener {
    override fun onDiscoveryStarted(serviceType: String) {}
    override fun onDiscoveryStopped(serviceType: String) {}

    override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
      synchronized(lock) {
        browsing = false
        discovery = null
        releaseMulticast()
      }
    }

    override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {}

    override fun onServiceFound(serviceInfo: NsdServiceInfo) {
      // The type sometimes comes back with a trailing dot.
      if (!serviceInfo.serviceType.startsWith(SERVICE_TYPE)) return
      synchronized(lock) {
        if (!browsing) return
        if (Build.VERSION.SDK_INT >= 34) watch(serviceInfo) else enqueue(serviceInfo)
      }
    }

    override fun onServiceLost(serviceInfo: NsdServiceInfo) {
      lost(serviceInfo.serviceName)
    }
  }

  private fun lost(name: String) {
    val removed = synchronized(lock) {
      pending.removeAll { it.serviceName == name }
      if (Build.VERSION.SDK_INT >= 34) infoCallbacks.remove(name)?.let { unregister(it) }
      known.remove(name) != null
    }
    if (removed) onLost(name)
  }

  /** Reads TXT `id` and `v`; without `id`, it is not a Signet computer. */
  private fun publish(name: String, addresses: List<InetAddress>, port: Int, txt: Map<String, ByteArray?>) {
    val id = txt["id"]?.toString(Charsets.UTF_8)?.takeIf { it.isNotEmpty() } ?: return
    val version = txt["v"]?.toString(Charsets.UTF_8)?.toIntOrNull() ?: 1
    val hosts = addresses.sortedBy { it is Inet6Address }.mapNotNull { it.hostAddress }
    if (hosts.isEmpty() || port <= 0) return
    val service = FoundService(name, id, version, hosts, port)
    synchronized(lock) {
      if (!browsing) return
      known[name] = service
    }
    onFound(service)
  }

  // region API 34+

  private fun watch(info: NsdServiceInfo) {
    val name = info.serviceName
    if (infoCallbacks.containsKey(name)) return
    val callback = object : NsdManager.ServiceInfoCallback {
      override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {
        synchronized(lock) { infoCallbacks.remove(name) }
      }

      override fun onServiceUpdated(serviceInfo: NsdServiceInfo) {
        publish(name, serviceInfo.hostAddresses, serviceInfo.port, serviceInfo.attributes)
      }

      override fun onServiceLost() {
        lost(name)
      }

      override fun onServiceInfoCallbackUnregistered() {}
    }
    try {
      nsd.registerServiceInfoCallback(info, callbackExecutor, callback)
      infoCallbacks[name] = callback
    } catch (_: Exception) {
    }
  }

  private fun unregister(callback: NsdManager.ServiceInfoCallback) {
    try {
      nsd.unregisterServiceInfoCallback(callback)
    } catch (_: Exception) {
    }
  }

  // endregion

  // region Before API 34: one resolution at a time

  private fun enqueue(info: NsdServiceInfo) {
    if (pending.none { it.serviceName == info.serviceName }) pending.add(info)
    resolveNext()
  }

  /** Called with `lock` held. */
  private fun resolveNext() {
    if (resolving || !browsing) return
    val info = pending.poll() ?: return
    resolving = true
    val seq = ++resolveSeq
    // If the system never answers, move on to the next one.
    handler.postDelayed({ synchronized(lock) { if (resolving && seq == resolveSeq) endResolve(null) } }, RESOLVE_TIMEOUT_MS)
    try {
      nsd.resolveService(info, object : NsdManager.ResolveListener {
        override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
          synchronized(lock) {
            if (seq != resolveSeq) return
            // FAILURE_ALREADY_ACTIVE: the system is busy, retry later.
            endResolve(if (errorCode == NsdManager.FAILURE_ALREADY_ACTIVE) serviceInfo else null)
          }
        }

        @Suppress("DEPRECATION")
        override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
          val host = serviceInfo.host
          if (host != null) publish(serviceInfo.serviceName, listOf(host), serviceInfo.port, serviceInfo.attributes)
          synchronized(lock) {
            if (seq == resolveSeq) endResolve(null)
          }
        }
      })
    } catch (_: Exception) {
      endResolve(null)
    }
  }

  /** Called with `lock` held. */
  private fun endResolve(retry: NsdServiceInfo?) {
    resolving = false
    resolveSeq++
    if (retry != null) {
      // Not right now: the system is busy.
      handler.postDelayed({ synchronized(lock) { if (browsing) { pending.add(retry); resolveNext() } } }, RETRY_DELAY_MS)
    }
    resolveNext()
  }

  // endregion

  companion object {
    private const val SERVICE_TYPE = "_signet._tcp"
    private const val RESOLVE_TIMEOUT_MS = 8000L
    private const val RETRY_DELAY_MS = 500L
  }
}
