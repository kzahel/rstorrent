package org.rstorrent.bootstrap

import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import java.io.Closeable
import org.rstorrent.bootstrap.uniffi.SafDescriptor
import org.rstorrent.bootstrap.uniffi.SafFileRole
import org.rstorrent.bootstrap.uniffi.SafStorage
import org.rstorrent.bootstrap.uniffi.SafStoragePlan

class ProductSafStorageHandles internal constructor(
    private val wanted: List<Pair<UInt, ParcelFileDescriptor>>,
    private val part: ParcelFileDescriptor,
    private val reopenedPart: ParcelFileDescriptor,
) : Closeable {
    fun storage(): SafStorage =
        SafStorage(
            wanted.map { (index, descriptor) -> SafDescriptor(index, descriptor.fd) },
            part.fd,
            reopenedPart.fd,
            emptyList(),
        )

    override fun close() {
        wanted.forEach { (_, descriptor) -> descriptor.close() }
        part.close()
        reopenedPart.close()
    }
}

class ProductSafPublishedHandles internal constructor(
    private val files: List<Pair<UInt, ParcelFileDescriptor>>,
) : Closeable {
    fun descriptors(): List<SafDescriptor> =
        files.map { (index, descriptor) -> SafDescriptor(index, descriptor.fd) }

    override fun close() {
        files.forEach { (_, descriptor) -> descriptor.close() }
    }
}

object ProductSafDocuments {
    private const val PREFERENCES = "product-saf"
    private const val TREE_URI = "tree-uri"
    private const val MIME_BINARY = "application/octet-stream"

    fun persistTree(
        context: Context,
        treeUri: Uri,
    ) {
        require(hasGrant(context, treeUri)) {
            "selected SAF tree has no persisted read/write grant"
        }
        check(
            context
                .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .putString(TREE_URI, treeUri.toString())
                .commit(),
        ) { "could not synchronously persist the SAF tree" }
    }

    fun selectedTree(context: Context): Uri? {
        val encoded = selectedTreeText(context) ?: return null
        val uri = Uri.parse(encoded)
        return uri.takeIf { hasGrant(context, it) }
    }

    fun releaseSelectedTreeForTest(context: Context) {
        check(isDebuggable(context)) { "SAF grant release is debug-only" }
        val uri = Uri.parse(requireNotNull(selectedTreeText(context)) { "no SAF tree is stored" })
        context.contentResolver.releasePersistableUriPermission(uri, GRANT_FLAGS)
        check(selectedTreeText(context) == uri.toString()) {
            "debug grant release must retain stale platform identity"
        }
        check(!hasGrant(context, uri)) { "SAF tree grant survived debug release" }
    }

    fun openStaging(
        context: Context,
        treeUri: Uri,
        plan: SafStoragePlan,
    ): ProductSafStorageHandles {
        require(plan.valid) { "native SAF plan rejected: ${plan.message}" }
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        val root = documentUri(treeUri)
        val stagingName = ".${plan.name}.rstorrent-staging"
        val partName = ".${plan.name}.rstorrent-parts"
        check(findUniqueChild(context, root, plan.name) == null) {
            "published SAF output already exists"
        }
        val staging =
            findUniqueChild(context, root, stagingName)
                ?: createDocument(
                    context,
                    root,
                    DocumentsContract.Document.MIME_TYPE_DIR,
                    stagingName,
                )
        val part =
            findUniqueChild(context, root, partName)
                ?: createDocument(context, root, MIME_BINARY, partName)
        val opened = mutableListOf<Pair<UInt, ParcelFileDescriptor>>()
        var partDescriptor: ParcelFileDescriptor? = null
        var reopenedPartDescriptor: ParcelFileDescriptor? = null
        try {
            for (file in plan.files.filter { it.role == SafFileRole.WANTED }) {
                val uri = createOrFindPath(context, staging, file.path)
                opened +=
                    file.fileIndex to
                        requireNotNull(context.contentResolver.openFileDescriptor(uri, "rw")) {
                            "provider refused a SAF payload descriptor"
                        }
            }
            partDescriptor =
                requireNotNull(context.contentResolver.openFileDescriptor(part, "rw")) {
                    "provider refused the SAF part descriptor"
                }
            reopenedPartDescriptor =
                requireNotNull(context.contentResolver.openFileDescriptor(part, "rw")) {
                    "provider refused an independent SAF part descriptor"
                }
            return ProductSafStorageHandles(
                opened,
                partDescriptor,
                reopenedPartDescriptor,
            )
        } catch (error: Throwable) {
            opened.forEach { (_, descriptor) -> descriptor.close() }
            partDescriptor?.close()
            reopenedPartDescriptor?.close()
            throw error
        }
    }

    fun publishAndOpen(
        context: Context,
        treeUri: Uri,
        plan: SafStoragePlan,
    ): ProductSafPublishedHandles {
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        val root = documentUri(treeUri)
        val staging = findUniqueChild(context, root, ".${plan.name}.rstorrent-staging")
        val existingFinal = findUniqueChild(context, root, plan.name)
        check(staging == null || existingFinal == null) {
            "both staging and published SAF outputs exist"
        }
        val published =
            existingFinal
                ?: requireNotNull(staging) { "SAF staging output is absent" }.let {
                    requireNotNull(
                        DocumentsContract.renameDocument(
                            context.contentResolver,
                            it,
                            plan.name,
                        ),
                    ) { "provider refused final SAF publication" }
                }
        val opened = mutableListOf<Pair<UInt, ParcelFileDescriptor>>()
        try {
            for (file in plan.files.filter { it.role == SafFileRole.WANTED }) {
                val uri = requirePath(context, published, file.path)
                opened +=
                    file.fileIndex to
                        requireNotNull(context.contentResolver.openFileDescriptor(uri, "r")) {
                            "provider refused a published SAF descriptor"
                        }
            }
            return ProductSafPublishedHandles(opened)
        } catch (error: Throwable) {
            opened.forEach { (_, descriptor) -> descriptor.close() }
            throw error
        }
    }

    private fun hasGrant(
        context: Context,
        treeUri: Uri,
    ): Boolean =
        context.contentResolver.persistedUriPermissions.any {
            it.uri == treeUri && it.isReadPermission && it.isWritePermission
        }

    private fun createOrFindPath(
        context: Context,
        root: Uri,
        components: List<String>,
    ): Uri {
        require(components.isNotEmpty()) { "SAF file path is empty" }
        var parent = root
        for (component in components.dropLast(1)) {
            parent =
                findUniqueChild(context, parent, component)
                    ?: createDocument(
                        context,
                        parent,
                        DocumentsContract.Document.MIME_TYPE_DIR,
                        component,
                    )
        }
        val name = components.last()
        return findUniqueChild(context, parent, name)
            ?: createDocument(context, parent, MIME_BINARY, name)
    }

    private fun requirePath(
        context: Context,
        root: Uri,
        components: List<String>,
    ): Uri {
        var current = root
        for (component in components) {
            current =
                requireNotNull(findUniqueChild(context, current, component)) {
                    "published SAF path is absent: ${components.joinToString("/")}"
                }
        }
        return current
    }

    private fun createDocument(
        context: Context,
        parent: Uri,
        mime: String,
        name: String,
    ): Uri =
        requireNotNull(
            DocumentsContract.createDocument(
                context.contentResolver,
                parent,
                mime,
                name,
            ),
        ) { "provider refused SAF document $name" }

    private fun findUniqueChild(
        context: Context,
        parent: Uri,
        name: String,
    ): Uri? {
        val parentId = DocumentsContract.getDocumentId(parent)
        val children =
            DocumentsContract.buildChildDocumentsUriUsingTree(parent, parentId)
        var match: Uri? = null
        context.contentResolver
            .query(
                children,
                arrayOf(
                    DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                    DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                ),
                null,
                null,
                null,
            )?.use { cursor ->
                while (cursor.moveToNext()) {
                    if (cursor.getString(1) != name) continue
                    check(match == null) { "provider returned duplicate child $name" }
                    match =
                        DocumentsContract.buildDocumentUriUsingTree(
                            parent,
                            cursor.getString(0),
                        )
                }
            }
        return match
    }

    private fun documentUri(treeUri: Uri): Uri =
        DocumentsContract.buildDocumentUriUsingTree(
            treeUri,
            DocumentsContract.getTreeDocumentId(treeUri),
        )

    val GRANT_FLAGS: Int =
        Intent.FLAG_GRANT_READ_URI_PERMISSION or
            Intent.FLAG_GRANT_WRITE_URI_PERMISSION

    fun isDebuggable(context: Context): Boolean =
        context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0

    private fun selectedTreeText(context: Context): String? =
        context
            .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(TREE_URI, null)
}
