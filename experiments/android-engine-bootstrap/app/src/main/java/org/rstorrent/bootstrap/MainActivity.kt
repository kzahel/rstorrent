package org.rstorrent.bootstrap

import android.Manifest
import android.annotation.SuppressLint
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.runtime.mutableStateOf
import org.rstorrent.bootstrap.ui.ProductApp

class MainActivity : ComponentActivity() {
    private var pendingCommand: Intent? = null
    private val productService = mutableStateOf<ProductEngineService?>(null)
    private var productBound = false
    private var productMode = false
    private var pendingProductMagnet: String? = null
    private var pendingProductTrackerPolicy: String? = null
    private var pendingProductEncryptionPolicy: String? = null
    private var pendingProductStartContent = true
    private var pendingProductTrackerEvidenceTorrent: String? = null
    private var pendingProductMseEvidence = false
    private var pendingProductDownloadAdmissionEvidence: String? = null
    private var pendingProductIpv6Policy: String? = null
    private var pendingCrashAfterSafRename = false
    private val productConnection =
        object : ServiceConnection {
            override fun onServiceConnected(
                name: ComponentName,
                binder: IBinder,
            ) {
                val service = (binder as ProductEngineService.LocalBinder).service
                ProductSafDocuments.selectedTree(this@MainActivity)?.let(service::setSafTree)
                productService.value = service
                if (pendingCrashAfterSafRename) {
                    pendingCrashAfterSafRename = false
                    service.enableCrashAfterSafRenameForTest()
                }
                pendingProductMagnet?.let {
                    pendingProductMagnet = null
                    val policy = pendingProductTrackerPolicy
                    pendingProductTrackerPolicy = null
                    val encryption = pendingProductEncryptionPolicy
                    pendingProductEncryptionPolicy = null
                    when {
                        policy != null -> service.addMagnetWithTrackerPolicyForTest(
                            it,
                            policy,
                            pendingProductStartContent,
                        )
                        encryption != null ->
                            service.addMagnetWithEncryptionPolicyForTest(it, encryption)
                        else -> service.addMagnet(it)
                    }
                    pendingProductStartContent = true
                }
                pendingProductTrackerEvidenceTorrent?.let {
                    pendingProductTrackerEvidenceTorrent = null
                    service.subscribeTrackerEvidenceForTest(it)
                }
                if (pendingProductMseEvidence) {
                    pendingProductMseEvidence = false
                    service.logMseDhEvidenceForTest()
                }
                pendingProductDownloadAdmissionEvidence?.let {
                    pendingProductDownloadAdmissionEvidence = null
                    service.logDownloadAdmissionEvidenceForTest(it)
                }
                pendingProductIpv6Policy?.let {
                    pendingProductIpv6Policy = null
                    android.util.Log.i("RSTorrentProduct", "ipv6_settings_bound mode=$it")
                    service.exerciseIpv6PolicyForTest(it)
                }
            }

            override fun onServiceDisconnected(name: ComponentName) {
                productService.value = null
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        route(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        route(intent)
    }

    override fun onStart() {
        super.onStart()
        if (productMode) bindProductService()
    }

    override fun onStop() {
        if (productBound) {
            unbindService(productConnection)
            productBound = false
            productService.value = null
        }
        super.onStop()
    }

    @SuppressLint("WrongConstant")
    override fun onActivityResult(
        requestCode: Int,
        resultCode: Int,
        data: Intent?,
    ) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != TREE_REQUEST) {
            if (requestCode == PRODUCT_TREE_REQUEST) {
                val treeUri = data?.data
                if (resultCode != RESULT_OK || treeUri == null) return
                val flags = data.flags and ProductSafDocuments.GRANT_FLAGS
                contentResolver.takePersistableUriPermission(treeUri, flags)
                ProductSafDocuments.persistTree(this, treeUri)
                productService.value?.setSafTree(treeUri)
            }
            return
        }
        val command = pendingCommand ?: error("SAF command was not retained")
        val treeUri = data?.data
        if (resultCode != RESULT_OK || treeUri == null) {
            finish()
            return
        }
        val flags =
            data.flags and (
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            )
        contentResolver.takePersistableUriPermission(treeUri, flags)
        command.putExtra("tree_uri", treeUri.toString())
        dispatch(command)
        pendingCommand = null
        finishIfRequested(command)
    }

    private fun route(command: Intent) {
        if (isDiagnostic(command)) {
            showDiagnosticSurface()
            handleDiagnostic(command)
        } else {
            showProductSurface(command)
        }
    }

    private fun showProductSurface(command: Intent) {
        if (
            ProductSafDocuments.isDebuggable(this) &&
            command.getBooleanExtra(EXTRA_PRODUCT_RELEASE_SAF_GRANT, false)
        ) {
            command.removeExtra(EXTRA_PRODUCT_RELEASE_SAF_GRANT)
            ProductSafDocuments.releaseSelectedTreeForTest(this)
        }
        if (
            ProductSafDocuments.isDebuggable(this) &&
            command.getBooleanExtra(EXTRA_PRODUCT_CRASH_AFTER_SAF_RENAME, false)
        ) {
            command.removeExtra(EXTRA_PRODUCT_CRASH_AFTER_SAF_RENAME)
            val service = productService.value
            if (service == null) {
                pendingCrashAfterSafRename = true
            } else {
                service.enableCrashAfterSafRenameForTest()
            }
        }
        if (!productMode) {
            productMode = true
            setContent {
                ProductApp(productService.value, ::launchProductTreePicker)
            }
        }
        if (ProductSafDocuments.isDebuggable(this)) {
            command
                .getStringExtra(EXTRA_PRODUCT_TRACKER_HTTPS_POLICY)
                ?.takeIf(String::isNotBlank)
                ?.let {
                    command.removeExtra(EXTRA_PRODUCT_TRACKER_HTTPS_POLICY)
                    pendingProductTrackerPolicy = it
                }
            command
                .getStringExtra(EXTRA_PRODUCT_TRACKER_EVIDENCE_TORRENT)
                ?.takeIf(String::isNotBlank)
                ?.let {
                    command.removeExtra(EXTRA_PRODUCT_TRACKER_EVIDENCE_TORRENT)
                    val service = productService.value
                    if (service == null) {
                        pendingProductTrackerEvidenceTorrent = it
                    } else {
                        service.subscribeTrackerEvidenceForTest(it)
                    }
                }
            command
                .getStringExtra(EXTRA_PRODUCT_ENCRYPTION_POLICY)
                ?.takeIf(String::isNotBlank)
                ?.let {
                    command.removeExtra(EXTRA_PRODUCT_ENCRYPTION_POLICY)
                    pendingProductEncryptionPolicy = it
                }
            if (command.getBooleanExtra(EXTRA_PRODUCT_MSE_EVIDENCE, false)) {
                command.removeExtra(EXTRA_PRODUCT_MSE_EVIDENCE)
                val service = productService.value
                if (service == null) {
                    pendingProductMseEvidence = true
                } else {
                    service.logMseDhEvidenceForTest()
                }
            }
            command
                .getStringExtra(EXTRA_PRODUCT_DOWNLOAD_ADMISSION_EVIDENCE)
                ?.takeIf(String::isNotBlank)
                ?.let {
                    command.removeExtra(EXTRA_PRODUCT_DOWNLOAD_ADMISSION_EVIDENCE)
                    val service = productService.value
                    if (service == null) {
                        pendingProductDownloadAdmissionEvidence = it
                    } else {
                        service.logDownloadAdmissionEvidenceForTest(it)
                    }
                }
            command
                .getStringExtra(EXTRA_PRODUCT_IPV6_POLICY)
                ?.takeIf(String::isNotBlank)
                ?.let {
                    command.removeExtra(EXTRA_PRODUCT_IPV6_POLICY)
                    android.util.Log.i("RSTorrentProduct", "ipv6_settings_intent mode=$it")
                    val service = productService.value
                    if (service == null) {
                        pendingProductIpv6Policy = it
                    } else {
                        service.exerciseIpv6PolicyForTest(it)
                    }
                }
        }
        command.getStringExtra(EXTRA_PRODUCT_MAGNET)?.takeIf(String::isNotBlank)?.let {
            command.removeExtra(EXTRA_PRODUCT_MAGNET)
            val startContent = command.getBooleanExtra(EXTRA_PRODUCT_START_CONTENT, true)
            command.removeExtra(EXTRA_PRODUCT_START_CONTENT)
            val service = productService.value
            if (service == null) {
                pendingProductMagnet = it
                pendingProductStartContent = startContent
            } else {
                val policy = pendingProductTrackerPolicy
                pendingProductTrackerPolicy = null
                val encryption = pendingProductEncryptionPolicy
                pendingProductEncryptionPolicy = null
                when {
                    policy != null ->
                        service.addMagnetWithTrackerPolicyForTest(it, policy, startContent)
                    encryption != null ->
                        service.addMagnetWithEncryptionPolicyForTest(it, encryption)
                    else -> service.addMagnet(it)
                }
            }
        }
        requestNotificationPermission()
        startProductService()
        if (
            ProductSafDocuments.isDebuggable(this) &&
            command.getBooleanExtra(EXTRA_PRODUCT_SELECT_SAF, false)
        ) {
            command.removeExtra(EXTRA_PRODUCT_SELECT_SAF)
            launchProductTreePicker()
        }
        if (lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.STARTED)) {
            bindProductService()
        }
    }

    private fun showDiagnosticSurface() {
        if (productMode) {
            productMode = false
            if (productBound) {
                unbindService(productConnection)
                productBound = false
                productService.value = null
            }
        }
        setContentView(
            TextView(this).apply {
                text = "RSTorrent engine bootstrap"
                textSize = 18f
                setPadding(32, 32, 32, 32)
            },
        )
    }

    private fun handleDiagnostic(command: Intent) {
        val storage = command.getStringExtra("storage") ?: "private"
        if (
            (command.action ?: BootstrapContract.ACTION_START) ==
                BootstrapContract.ACTION_START &&
            storage.startsWith("saf-") &&
            command.getStringExtra("tree_uri") == null
        ) {
            pendingCommand = Intent(command)
            val picker =
                Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
                    addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
                    addFlags(Intent.FLAG_GRANT_PREFIX_URI_PERMISSION)
                    val initial =
                        command.getStringExtra("tree_initial_uri")
                            ?: "content://com.android.externalstorage.documents/document/primary%3ADownload"
                    putExtra(
                        "android.provider.extra.INITIAL_URI",
                        android.net.Uri.parse(initial),
                    )
                }
            startActivityForResult(picker, TREE_REQUEST)
            return
        }
        dispatch(command)
        finishIfRequested(command)
    }

    private fun startProductService() {
        val serviceIntent = Intent(this, ProductEngineService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(serviceIntent)
        } else {
            startService(serviceIntent)
        }
    }

    private fun launchProductTreePicker() {
        startActivityForResult(
            Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
                addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
                addFlags(Intent.FLAG_GRANT_PREFIX_URI_PERMISSION)
                putExtra(
                    "android.provider.extra.INITIAL_URI",
                    android.net.Uri.parse(
                        "content://com.android.externalstorage.documents/document/" +
                            "primary%3ADownload",
                    ),
                )
            },
            PRODUCT_TREE_REQUEST,
        )
    }

    private fun bindProductService() {
        if (productBound) return
        productBound =
            bindService(
                Intent(this, ProductEngineService::class.java),
                productConnection,
                Context.BIND_AUTO_CREATE,
            )
    }

    private fun requestNotificationPermission() {
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 52)
        }
    }

    private fun isDiagnostic(command: Intent): Boolean =
        command.action in
            setOf(
                BootstrapContract.ACTION_START,
                BootstrapContract.ACTION_CANCEL,
                BootstrapContract.ACTION_OBSERVE,
                BootstrapContract.ACTION_VERIFY,
            )

    private fun finishIfRequested(command: Intent) {
        if (command.getBooleanExtra("finish_activity", false)) {
            finish()
        }
    }

    private fun dispatch(command: Intent) {
        val serviceIntent =
            Intent(this, EngineService::class.java).apply {
                action = command.action ?: BootstrapContract.ACTION_START
                replaceExtras(command.extras ?: Bundle())
            }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(serviceIntent)
        } else {
            startService(serviceIntent)
        }
    }

    companion object {
        private const val TREE_REQUEST = 51
        private const val PRODUCT_TREE_REQUEST = 53
        const val EXTRA_PRODUCT_MAGNET = "product_magnet"
        const val EXTRA_PRODUCT_TRACKER_HTTPS_POLICY = "product_tracker_https_policy"
        const val EXTRA_PRODUCT_ENCRYPTION_POLICY = "product_encryption_policy"
        const val EXTRA_PRODUCT_MSE_EVIDENCE = "product_mse_evidence"
        const val EXTRA_PRODUCT_DOWNLOAD_ADMISSION_EVIDENCE =
            "product_download_admission_evidence"
        const val EXTRA_PRODUCT_IPV6_POLICY = "product_ipv6_policy"
        const val EXTRA_PRODUCT_START_CONTENT = "product_start_content"
        const val EXTRA_PRODUCT_TRACKER_EVIDENCE_TORRENT = "product_tracker_evidence_torrent"
        const val EXTRA_PRODUCT_SELECT_SAF = "product_select_saf"
        const val EXTRA_PRODUCT_RELEASE_SAF_GRANT = "product_release_saf_grant"
        const val EXTRA_PRODUCT_CRASH_AFTER_SAF_RENAME =
            "product_crash_after_saf_rename"
    }
}
