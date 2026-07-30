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
    private val productConnection =
        object : ServiceConnection {
            override fun onServiceConnected(
                name: ComponentName,
                binder: IBinder,
            ) {
                val service = (binder as ProductEngineService.LocalBinder).service
                productService.value = service
                pendingProductMagnet?.let {
                    pendingProductMagnet = null
                    service.addMagnet(it)
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
        if (!productMode) {
            productMode = true
            setContent {
                ProductApp(productService.value)
            }
        }
        command.getStringExtra(EXTRA_PRODUCT_MAGNET)?.takeIf(String::isNotBlank)?.let {
            command.removeExtra(EXTRA_PRODUCT_MAGNET)
            val service = productService.value
            if (service == null) {
                pendingProductMagnet = it
            } else {
                service.addMagnet(it)
            }
        }
        requestNotificationPermission()
        startProductService()
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
        const val EXTRA_PRODUCT_MAGNET = "product_magnet"
    }
}
