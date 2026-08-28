package org.rstorrent.bootstrap

import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.os.CancellationSignal
import android.provider.DocumentsContract
import java.security.MessageDigest
import org.rstorrent.bootstrap.uniffi.SafDynamicFileRole
import org.rstorrent.bootstrap.uniffi.SafRemovalNamespace
import org.rstorrent.bootstrap.uniffi.SafRemovalPlan
import org.rstorrent.bootstrap.uniffi.SafStorageAccess
import org.rstorrent.bootstrap.uniffi.SafStorageFailureKind
import org.rstorrent.bootstrap.uniffi.SafStorageObjectKind
import org.rstorrent.bootstrap.uniffi.SafStorageObservation
import org.rstorrent.bootstrap.uniffi.SafStorageRequest

class SafStorageRequestException(
    val kind: SafStorageFailureKind,
    message: String,
) : IllegalStateException(message)

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
        torrentId: String,
        name: String,
    ) {
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        requireValidComponent(torrentId)
        requireValidComponent(name)
        val root = documentUri(treeUri)
        val staging = findUniqueChild(context, root, ".$torrentId.rstorrent-staging")
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

    fun deleteManaged(
        context: Context,
        treeUri: Uri,
        plan: SafRemovalPlan,
    ) {
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        val root = documentUri(treeUri)
        deleteManagedArtifacts(
            name = plan.name,
            torrentId = plan.torrentId,
            namespace = plan.namespace,
            tree = plan.tree,
            files = plan.files.map { it.components },
            directories = plan.directories.map { it.components },
            root = root,
            find = { parent, name -> findUniqueChild(context, parent, name) },
            kind = { document -> observeDocument(context, document).kind!! },
            isEmptyDirectory = { document -> isEmptyDirectory(context, document) },
            delete = { document ->
                DocumentsContract.deleteDocument(context.contentResolver, document)
            },
        )
    }

    fun publishedDocument(
        context: Context,
        treeUri: Uri,
        path: List<String>,
    ): Uri? {
        require(hasGrant(context, treeUri)) { "persisted SAF grant is unavailable" }
        require(path.isNotEmpty()) { "published path is empty" }
        path.forEach(::requireValidComponent)
        var current = documentUri(treeUri)
        for (component in path) {
            current = findUniqueChild(context, current, component) ?: return null
        }
        return current
    }

    private fun hasGrant(
        context: Context,
        treeUri: Uri,
    ): Boolean =
        context.contentResolver.persistedUriPermissions.any {
            it.uri == treeUri && it.isReadPermission && it.isWritePermission
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

    private fun isEmptyDirectory(
        context: Context,
        document: Uri,
    ): Boolean {
        val documentId = DocumentsContract.getDocumentId(document)
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(document, documentId)
        return context.contentResolver
            .query(
                children,
                arrayOf(DocumentsContract.Document.COLUMN_DOCUMENT_ID),
                null,
                null,
                null,
            )?.use { cursor -> !cursor.moveToFirst() }
            ?: throw SafStorageRequestException(
                SafStorageFailureKind.PROVIDER_REFUSED,
                "provider refused SAF directory enumeration",
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

internal fun <T> deleteManagedArtifacts(
    name: String,
    torrentId: String,
    namespace: SafRemovalNamespace,
    tree: Boolean,
    files: List<List<String>>,
    directories: List<List<String>>,
    root: T,
    find: (T, String) -> T?,
    kind: (T) -> SafStorageObjectKind,
    isEmptyDirectory: (T) -> Boolean,
    delete: (T) -> Boolean,
) {
    val stagingName = ".$torrentId.rstorrent-staging"
    val partName = ".$torrentId.rstorrent-parts"
    val staging =
        if (namespace == SafRemovalNamespace.LEGACY ||
            namespace == SafRemovalNamespace.STAGING ||
            namespace == SafRemovalNamespace.PUBLISHING ||
            namespace == SafRemovalNamespace.PUBLISHED
        ) {
            find(root, stagingName)
        } else {
            null
        }
    val publication =
        if (namespace == SafRemovalNamespace.PUBLISHING ||
            namespace == SafRemovalNamespace.PUBLISHED
        ) {
            find(root, name)
        } else {
            null
        }
    val namespaceDocuments =
        when (namespace) {
            SafRemovalNamespace.NONE -> emptyList()
            SafRemovalNamespace.LEGACY -> listOfNotNull(find(root, torrentId), staging)
            SafRemovalNamespace.STAGING -> listOfNotNull(staging)
            SafRemovalNamespace.PUBLISHING -> {
                check(staging == null || publication == null) {
                    "both staging and published SAF outputs exist"
                }
                listOfNotNull(publication ?: staging)
            }
            SafRemovalNamespace.PUBLISHED -> {
                check(staging == null) { "published SAF storage has a staging artifact" }
                listOfNotNull(publication)
            }
        }
    val part =
        if (namespace == SafRemovalNamespace.NONE) {
            null
        } else {
            find(root, partName)
        }
    part?.let {
        check(kind(it) == SafStorageObjectKind.FILE) {
            "managed SAF part artifact is not a file"
        }
    }
    namespaceDocuments.forEach {
        val expected = if (tree) SafStorageObjectKind.DIRECTORY else SafStorageObjectKind.FILE
        check(kind(it) == expected) { "managed SAF namespace has an unexpected type" }
    }

    fun resolve(namespaceDocument: T, components: List<String>): T? {
        var current = namespaceDocument
        for ((index, component) in components.withIndex()) {
            current = find(current, component) ?: return null
            if (index + 1 < components.size) {
                check(kind(current) == SafStorageObjectKind.DIRECTORY) {
                    "managed SAF payload parent has an unexpected type"
                }
            }
        }
        return current
    }

    val payloadFiles =
        if (tree) {
            namespaceDocuments.flatMap { namespaceDocument ->
                files.mapNotNull { components ->
                    resolve(namespaceDocument, components)?.also { document ->
                        check(kind(document) == SafStorageObjectKind.FILE) {
                            "managed SAF payload leaf has an unexpected type"
                        }
                    }
                }
            }
        } else {
            namespaceDocuments
        }
    val payloadDirectories =
        if (tree) {
            namespaceDocuments.flatMap { namespaceDocument ->
                directories.mapNotNull { components ->
                    resolve(namespaceDocument, components)?.also { document ->
                        check(kind(document) == SafStorageObjectKind.DIRECTORY) {
                            "managed SAF payload directory has an unexpected type"
                        }
                    }
                }
            }
        } else {
            emptyList()
        }
    for (document in payloadFiles) {
        check(delete(document)) { "provider refused managed SAF payload deletion" }
    }
    if (part != null) {
        check(delete(part)) { "provider refused managed SAF part deletion" }
    }
    for (directory in payloadDirectories) {
        if (isEmptyDirectory(directory)) {
            check(delete(directory)) { "provider refused empty SAF directory deletion" }
        }
    }
}
