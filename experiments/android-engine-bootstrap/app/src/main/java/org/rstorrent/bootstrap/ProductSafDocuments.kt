package org.rstorrent.bootstrap

import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.os.CancellationSignal
import android.provider.DocumentsContract
import java.io.Closeable
import java.security.MessageDigest
import org.rstorrent.bootstrap.uniffi.SafDescriptor
import org.rstorrent.bootstrap.uniffi.SafDynamicFileRole
import org.rstorrent.bootstrap.uniffi.SafFileRole
import org.rstorrent.bootstrap.uniffi.SafRemovalPlan
import org.rstorrent.bootstrap.uniffi.SafStorage
import org.rstorrent.bootstrap.uniffi.SafStorageAccess
import org.rstorrent.bootstrap.uniffi.SafStorageFailureKind
import org.rstorrent.bootstrap.uniffi.SafStorageObjectKind
import org.rstorrent.bootstrap.uniffi.SafStorageObservation
import org.rstorrent.bootstrap.uniffi.SafStoragePlan
import org.rstorrent.bootstrap.uniffi.SafStorageRequest

class SafStorageRequestException(
    val kind: SafStorageFailureKind,
    message: String,
) : IllegalStateException(message)

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

    fun openDynamic(
        context: Context,
        treeUri: Uri,
        request: SafStorageRequest,
        cancellation: CancellationSignal,
    ): ParcelFileDescriptor {
        require(hasGrant(context, treeUri)) {
            "persisted SAF grant is unavailable"
        }
        require(request.path.isNotEmpty()) { "SAF request path is empty" }
        request.path.forEach(::requireValidComponent)
        val create = request.access == SafStorageAccess.READ_WRITE_CREATE
        var parent = documentUri(treeUri)
        for (component in request.path.dropLast(1)) {
            parent =
                findUniqueChild(context, parent, component)
                    ?: if (create) {
                        synchronized(this) {
                            findUniqueChild(context, parent, component)
                                ?: createDocument(
                                    context,
                                    parent,
                                    DocumentsContract.Document.MIME_TYPE_DIR,
                                    component,
                                )
                        }
                    } else {
                        throw SafStorageRequestException(
                            SafStorageFailureKind.MISSING,
                            "managed SAF parent is absent",
                        )
                    }
        }
        val name = request.path.last()
        val document =
            findUniqueChild(context, parent, name)
                ?: if (create) {
                    synchronized(this) {
                        findUniqueChild(context, parent, name)
                            ?: createDocument(context, parent, MIME_BINARY, name)
                    }
                } else {
                    throw SafStorageRequestException(
                        SafStorageFailureKind.MISSING,
                        "managed SAF file is absent",
                    )
                }
        val mode =
            when (request.access) {
                SafStorageAccess.READ_EXISTING -> "r"
                SafStorageAccess.READ_WRITE_EXISTING,
                SafStorageAccess.READ_WRITE_CREATE,
                -> "rw"
            }
        return requireNotNull(
            context.contentResolver.openFileDescriptor(document, mode, cancellation),
        ) { "provider refused a SAF descriptor" }
    }

    fun deleteDynamic(
        context: Context,
        treeUri: Uri,
        request: SafStorageRequest,
    ) {
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        require(request.role == SafDynamicFileRole.PART) {
            "only the managed part file may be deleted dynamically"
        }
        require(request.path.isNotEmpty()) { "SAF request path is empty" }
        request.path.forEach(::requireValidComponent)
        var current = documentUri(treeUri)
        for (component in request.path) {
            current = findUniqueChild(context, current, component) ?: return
        }
        check(DocumentsContract.deleteDocument(context.contentResolver, current)) {
            "provider refused managed SAF file deletion"
        }
    }

    fun observeDynamic(
        context: Context,
        treeUri: Uri,
        request: SafStorageRequest,
    ): SafStorageObservation {
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        request.path.forEach(::requireValidComponent)
        var current = documentUri(treeUri)
        for (component in request.path) {
            current =
                findUniqueChild(context, current, component)
                    ?: return SafStorageObservation(false, null, null, null)
        }
        return observeDocument(context, current)
    }

    fun publish(
        context: Context,
        treeUri: Uri,
        name: String,
    ) {
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        requireValidComponent(name)
        val root = documentUri(treeUri)
        val staging = findUniqueChild(context, root, ".$name.rstorrent-staging")
        val existingFinal = findUniqueChild(context, root, name)
        check(staging == null || existingFinal == null) {
            "both staging and published SAF outputs exist"
        }
        if (existingFinal == null) {
            requireNotNull(staging) { "SAF staging output is absent" }
            requireNotNull(
                DocumentsContract.renameDocument(
                    context.contentResolver,
                    staging,
                    name,
                ),
            ) { "provider refused final SAF publication" }
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

    fun deleteManaged(
        context: Context,
        treeUri: Uri,
        plan: SafRemovalPlan,
    ) {
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        val root = documentUri(treeUri)
        deleteManagedArtifacts(
            plan.name,
            find = { name -> findUniqueChild(context, root, name) },
            delete = { document ->
                DocumentsContract.deleteDocument(context.contentResolver, document)
            },
        )
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

    private fun requireValidComponent(component: String) {
        require(
            component.isNotEmpty() &&
                component != "." &&
                component != ".." &&
                component.length <= 255 &&
                component.none { it == '/' || it == '\\' || it == '\u0000' },
        ) { "invalid SAF path component" }
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
                    if (match != null) {
                        throw SafStorageRequestException(
                            SafStorageFailureKind.NAME_COLLISION,
                            "provider returned duplicate child $name",
                        )
                    }
                    match =
                        DocumentsContract.buildDocumentUriUsingTree(
                            parent,
                            cursor.getString(0),
                        )
                }
            }
        return match
    }

    private fun observeDocument(
        context: Context,
        document: Uri,
    ): SafStorageObservation {
        val projection =
            arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE,
                DocumentsContract.Document.COLUMN_LAST_MODIFIED,
            )
        return context.contentResolver.query(document, projection, null, null, null)?.use { cursor ->
            if (!cursor.moveToFirst()) {
                return@use SafStorageObservation(false, null, null, null)
            }
            check(!cursor.moveToNext()) { "provider returned duplicate document observation" }
            cursor.moveToFirst()
            val documentId = cursor.getString(0)
            val mime = cursor.getString(1)
            val kind =
                when {
                    mime == DocumentsContract.Document.MIME_TYPE_DIR ->
                        SafStorageObjectKind.DIRECTORY
                    mime != null -> SafStorageObjectKind.FILE
                    else -> SafStorageObjectKind.OTHER
                }
            val length =
                if (kind == SafStorageObjectKind.FILE && !cursor.isNull(2)) {
                    cursor.getLong(2).takeIf { it >= 0 }?.toULong()
                } else {
                    null
                }
            val modified = if (cursor.isNull(3)) null else cursor.getLong(3)
            val tokenMaterial =
                listOf(
                    "saf-observation-v1",
                    documentId,
                    mime ?: "unknown",
                    length?.toString() ?: "unknown",
                    modified?.toString() ?: "unknown",
                ).joinToString("\u0000")
            val token =
                MessageDigest
                    .getInstance("SHA-256")
                    .digest(tokenMaterial.toByteArray(Charsets.UTF_8))
                    .joinToString("") { byte -> "%02x".format(byte.toInt() and 0xff) }
            SafStorageObservation(true, kind, length, "saf-v1:$token")
        } ?: throw SafStorageRequestException(
            SafStorageFailureKind.PROVIDER_REFUSED,
            "provider refused SAF document observation",
        )
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

internal fun managedRemovalNames(name: String): List<String> =
    listOf(name, ".$name.rstorrent-staging", ".$name.rstorrent-parts")

internal fun <T> deleteManagedArtifacts(
    name: String,
    find: (String) -> T?,
    delete: (T) -> Boolean,
) {
    for (artifact in managedRemovalNames(name)) {
        val document = find(artifact) ?: continue
        check(delete(document)) { "provider refused to delete SAF document $artifact" }
    }
}
