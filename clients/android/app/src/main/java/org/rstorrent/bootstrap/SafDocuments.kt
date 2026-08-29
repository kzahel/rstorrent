package org.rstorrent.bootstrap

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import java.security.MessageDigest
import org.json.JSONArray
import org.json.JSONObject
import org.rstorrent.bootstrap.uniffi.EngineSession
import org.rstorrent.bootstrap.uniffi.SafDescriptor
import org.rstorrent.bootstrap.uniffi.SafFileRole
import org.rstorrent.bootstrap.uniffi.SafStorage
import org.rstorrent.bootstrap.uniffi.SafStoragePlan
import org.rstorrent.bootstrap.uniffi.StartResult
import org.rstorrent.bootstrap.uniffi.inspectBorrowedDescriptor
import org.rstorrent.bootstrap.uniffi.safStoragePlan

data class DirectSafRun(
    val treeUri: Uri,
    val plan: SafStoragePlan,
    val contentUri: Uri,
    val partUri: Uri,
    val wantedUris: Map<UInt, Uri>,
    private val descriptors: List<ParcelFileDescriptor>,
    private val partDescriptor: ParcelFileDescriptor,
    private val reopenedPartDescriptor: ParcelFileDescriptor,
) {
    fun start(
        session: EngineSession,
        config: org.rstorrent.bootstrap.uniffi.EngineConfig,
    ): StartResult {
        try {
            return session.startSaf(
                config,
                SafStorage(
                    wantedUris.map { (index, _) ->
                        SafDescriptor(index, descriptorFor(index).fd)
                    },
                    partDescriptor.fd,
                    reopenedPartDescriptor.fd,
                ),
            )
        } finally {
            descriptors.forEach(ParcelFileDescriptor::close)
            partDescriptor.close()
            reopenedPartDescriptor.close()
        }
    }

    private fun descriptorFor(index: UInt): ParcelFileDescriptor {
        val ordered = wantedUris.keys.toList()
        return descriptors[ordered.indexOf(index)]
    }
}

object SafDocuments {
    private const val PREFERENCES = "saf-run"
    private const val STATE = "state"
    private const val MIME_BINARY = "application/octet-stream"

    fun prepare(
        context: Context,
        treeUri: Uri,
        metainfo: ByteArray,
        skipFiles: List<UInt>,
    ): DirectSafRun {
        val resolver = context.contentResolver
        val plan = safStoragePlan(metainfo, skipFiles)
        check(plan.valid) { "native SAF plan rejected: ${plan.message}" }
        val grantRoot = documentUri(treeUri)
        val partName = ".diagnostic-${plan.infoHashHex}.rstorrent-parts"
        check(findChild(context, grantRoot, plan.name) == null) {
            "final SAF output already exists"
        }
        check(findChild(context, grantRoot, partName) == null) {
            "SAF part document already exists"
        }

        var content: Uri? = null
        var part: Uri? = null
        val opened = mutableListOf<ParcelFileDescriptor>()
        var partDescriptor: ParcelFileDescriptor? = null
        var reopenedPartDescriptor: ParcelFileDescriptor? = null
        try {
            content =
                if (plan.tree) {
                    requireNotNull(
                        DocumentsContract.createDocument(
                            resolver,
                            grantRoot,
                            DocumentsContract.Document.MIME_TYPE_DIR,
                            plan.name,
                        ),
                    ) { "provider refused the SAF content directory" }
                } else {
                    null
                }
            part =
                requireNotNull(
                    DocumentsContract.createDocument(
                        resolver,
                        grantRoot,
                        MIME_BINARY,
                        partName,
                    ),
                ) { "provider refused the SAF part document" }
            val wanted = linkedMapOf<UInt, Uri>()
            for (file in plan.files) {
                if (file.role == SafFileRole.WANTED) {
                    val components = if (plan.tree) file.path else listOf(plan.name)
                    wanted[file.fileIndex] = createPath(context, grantRoot, components)
                }
            }
            for (uri in wanted.values) {
                opened +=
                    requireNotNull(resolver.openFileDescriptor(uri, "rw")) {
                        "provider refused a SAF payload descriptor"
                    }
            }
            partDescriptor =
                requireNotNull(resolver.openFileDescriptor(part, "rw")) {
                    "provider refused the SAF part descriptor"
                }
            reopenedPartDescriptor =
                requireNotNull(resolver.openFileDescriptor(part, "rw")) {
                    "provider refused an independent SAF part descriptor"
                }
            val directContent = content ?: wanted.values.single()
            return DirectSafRun(
                treeUri,
                plan,
                directContent,
                part,
                wanted,
                opened,
                partDescriptor,
                reopenedPartDescriptor,
            )
        } catch (error: Throwable) {
            opened.forEach(ParcelFileDescriptor::close)
            partDescriptor?.close()
            reopenedPartDescriptor?.close()
            content?.let { delete(context, it) }
            part?.let { delete(context, it) }
            releaseGrant(context, treeUri)
            throw error
        }
    }

    fun persistCompleted(
        context: Context,
        run: DirectSafRun,
    ): JSONObject {
        val resolver = context.contentResolver
        val manifest = JSONArray()
        for ((fileIndex, uri) in run.wantedUris) {
            val planFile = run.plan.files.single { it.fileIndex == fileIndex }
            val digest = MessageDigest.getInstance("SHA-1")
            var length = 0L
            requireNotNull(resolver.openInputStream(uri)) { "provider refused content read" }
                .use { input ->
                    val buffer = ByteArray(16 * 1024)
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        digest.update(buffer, 0, read)
                        length += read
                    }
                }
            check(length.toULong() == planFile.length) { "completed SAF file length changed" }
            manifest.put(
                JSONObject()
                    .put("file_index", fileIndex.toLong())
                    .put("path", JSONArray(if (run.plan.tree) planFile.path else emptyList<String>()))
                    .put("length", length)
                    .put(
                        "sha1",
                        digest.digest().joinToString("") { "%02x".format(it.toInt() and 0xff) },
                    ),
            )
        }
        val state =
            JSONObject()
                .put("run_id", "")
                .put("tree_uri", run.treeUri.toString())
                .put("content_uri", run.contentUri.toString())
                .put("part_uri", run.partUri.toString())
                .put("manifest", manifest)
        check(
            context
                .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .putString(STATE, state.toString())
                .commit(),
        ) { "could not synchronously persist SAF restart state" }
        return JSONObject()
            .put("content_uri", run.contentUri.toString())
            .put("part_uri", run.partUri.toString())
            .put("file_count", manifest.length())
    }

    fun bindRunId(
        context: Context,
        runId: String,
    ) {
        val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        val state = JSONObject(requireNotNull(preferences.getString(STATE, null)))
        state.put("run_id", runId)
        check(preferences.edit().putString(STATE, state.toString()).commit()) {
            "could not synchronously bind the SAF run ID"
        }
    }

    fun verifyAndCleanup(
        context: Context,
        runId: String,
    ): JSONObject {
        val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        val state = JSONObject(requireNotNull(preferences.getString(STATE, null)) {
            "no persisted SAF verification state"
        })
        check(state.getString("run_id") == runId) { "persisted SAF run ID differs" }
        val treeUri = Uri.parse(state.getString("tree_uri"))
        val content = Uri.parse(state.getString("content_uri"))
        val part = Uri.parse(state.getString("part_uri"))
        val verified = JSONArray()
        val manifest = state.getJSONArray("manifest")
        for (position in 0 until manifest.length()) {
            val expected = manifest.getJSONObject(position)
            val components =
                expected
                    .getJSONArray("path")
                    .let { array ->
                        (0 until array.length()).map(array::getString)
                    }
            val uri = if (components.isEmpty()) content else requirePath(context, content, components)
            val descriptor =
                requireNotNull(context.contentResolver.openFileDescriptor(uri, "r")) {
                    "provider refused restart descriptor"
                }
            val inspection =
                descriptor.use {
                    inspectBorrowedDescriptor(
                        it.fd,
                        expected.getLong("length").toULong(),
                        expected.getString("sha1"),
                    )
                }
            check(inspection.success) {
                "Rust restart verification failed for ${components.joinToString("/")}: " +
                    inspection.message
            }
            verified.put(
                JSONObject()
                    .put("file_index", expected.getLong("file_index"))
                    .put("path", components.joinToString("/"))
                    .put("length", inspection.length.toLong())
                    .put("sha1", inspection.sha1Hex)
                    .put("allocated_bytes", inspection.allocatedBytes.toLong()),
            )
        }
        check(delete(context, content)) { "could not delete direct SAF content" }
        check(delete(context, part)) { "could not delete SAF part document" }
        try {
            context.contentResolver.releasePersistableUriPermission(
                treeUri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
        } catch (_: SecurityException) {
            // A revoked grant is already released, but verification above must
            // have succeeded before this cleanup path is accepted.
        }
        check(preferences.edit().clear().commit()) {
            "could not clear completed SAF restart state"
        }
        return JSONObject()
            .put("status", "SUCCEEDED")
            .put("verified_files", verified)
            .put("content_deleted", true)
            .put("part_deleted", true)
    }

    fun cleanup(
        context: Context,
        run: DirectSafRun,
    ) {
        delete(context, run.contentUri)
        delete(context, run.partUri)
        releaseGrant(context, run.treeUri)
    }

    private fun createPath(
        context: Context,
        root: Uri,
        components: List<String>,
    ): Uri {
        check(components.isNotEmpty()) { "SAF file path is empty" }
        var parent = root
        for (component in components.dropLast(1)) {
            val existing = findChild(context, parent, component)
            parent =
                existing ?: requireNotNull(
                    DocumentsContract.createDocument(
                        context.contentResolver,
                        parent,
                        DocumentsContract.Document.MIME_TYPE_DIR,
                        component,
                    ),
                ) { "provider refused directory $component" }
        }
        val finalName = components.last()
        check(findChild(context, parent, finalName) == null) {
            "SAF document already exists: $finalName"
        }
        return requireNotNull(
            DocumentsContract.createDocument(
                context.contentResolver,
                parent,
                MIME_BINARY,
                finalName,
            ),
        ) { "provider refused document $finalName" }
    }

    private fun requirePath(
        context: Context,
        root: Uri,
        components: List<String>,
    ): Uri {
        var current = root
        for (component in components) {
            current =
                requireNotNull(findChild(context, current, component)) {
                    "direct SAF path is absent: ${components.joinToString("/")}"
                }
        }
        return current
    }

    private fun findChild(
        context: Context,
        parent: Uri,
        name: String,
    ): Uri? {
        val parentId = DocumentsContract.getDocumentId(parent)
        val children =
            DocumentsContract.buildChildDocumentsUriUsingTree(parent, parentId)
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
                    if (cursor.getString(1) == name) {
                        return DocumentsContract.buildDocumentUriUsingTree(
                            parent,
                            cursor.getString(0),
                        )
                    }
                }
            }
        return null
    }

    private fun documentUri(treeUri: Uri): Uri =
        DocumentsContract.buildDocumentUriUsingTree(
            treeUri,
            DocumentsContract.getTreeDocumentId(treeUri),
        )

    private fun delete(
        context: Context,
        uri: Uri,
    ): Boolean =
        try {
            DocumentsContract.deleteDocument(context.contentResolver, uri)
        } catch (_: Throwable) {
            false
        }

    private fun releaseGrant(
        context: Context,
        treeUri: Uri,
    ) {
        try {
            context.contentResolver.releasePersistableUriPermission(
                treeUri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
        } catch (_: SecurityException) {
            // The permission may have been revoked by the adverse test.
        }
    }
}
