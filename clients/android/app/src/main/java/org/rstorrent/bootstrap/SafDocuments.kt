package org.rstorrent.bootstrap

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import org.json.JSONArray
import org.json.JSONObject
import org.rstorrent.bootstrap.uniffi.EngineReport
import org.rstorrent.bootstrap.uniffi.EngineSession
import org.rstorrent.bootstrap.uniffi.SafDescriptor
import org.rstorrent.bootstrap.uniffi.SafFileRole
import org.rstorrent.bootstrap.uniffi.SafStorage
import org.rstorrent.bootstrap.uniffi.SafStoragePlan
import org.rstorrent.bootstrap.uniffi.StartResult
import org.rstorrent.bootstrap.uniffi.inspectBorrowedDescriptor
import org.rstorrent.bootstrap.uniffi.safStoragePlan

data class PreparedSafRun(
    val treeUri: Uri,
    val plan: SafStoragePlan,
    val stagingUri: Uri,
    val partUri: Uri,
    val wantedUris: Map<UInt, Uri>,
    val materializationUris: Map<UInt, Uri>,
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
                        SafDescriptor(index, descriptorFor(index, false).fd)
                    },
                    partDescriptor.fd,
                    reopenedPartDescriptor.fd,
                    materializationUris.map { (index, _) ->
                        SafDescriptor(index, descriptorFor(index, true).fd)
                    },
                ),
            )
        } finally {
            descriptors.forEach(ParcelFileDescriptor::close)
            partDescriptor.close()
            reopenedPartDescriptor.close()
        }
    }

    private fun descriptorFor(
        index: UInt,
        materialization: Boolean,
    ): ParcelFileDescriptor {
        val ordered =
            if (materialization) {
                materializationUris.keys.toList()
            } else {
                wantedUris.keys.toList()
            }
        return descriptors[
            (if (materialization) wantedUris.size else 0) + ordered.indexOf(index)
        ]
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
        materializeFiles: List<UInt>,
    ): PreparedSafRun {
        val resolver = context.contentResolver
        val plan = safStoragePlan(metainfo, skipFiles, materializeFiles)
        check(plan.valid) { "native SAF plan rejected: ${plan.message}" }
        val grantRoot = documentUri(treeUri)
        val stagingName = ".${plan.name}.rstorrent-staging"
        val partName = ".${plan.name}.rstorrent-parts"
        check(findChild(context, grantRoot, plan.name) == null) {
            "final SAF output already exists"
        }
        check(findChild(context, grantRoot, stagingName) == null) {
            "SAF staging directory already exists"
        }
        check(findChild(context, grantRoot, partName) == null) {
            "SAF part document already exists"
        }

        var staging: Uri? = null
        var part: Uri? = null
        val opened = mutableListOf<ParcelFileDescriptor>()
        var partDescriptor: ParcelFileDescriptor? = null
        var reopenedPartDescriptor: ParcelFileDescriptor? = null
        try {
            staging =
                requireNotNull(
                    DocumentsContract.createDocument(
                        resolver,
                        grantRoot,
                        DocumentsContract.Document.MIME_TYPE_DIR,
                        stagingName,
                    ),
                ) { "provider refused the SAF staging directory" }
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
            val materializations = linkedMapOf<UInt, Uri>()
            for (file in plan.files) {
                when {
                    file.role == SafFileRole.WANTED ->
                        wanted[file.fileIndex] =
                            createPath(context, staging, file.path, temporary = false)
                    file.materialize ->
                        materializations[file.fileIndex] =
                            createPath(context, staging, file.path, temporary = true)
                }
            }
            for (uri in wanted.values + materializations.values) {
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
            return PreparedSafRun(
                treeUri,
                plan,
                staging,
                part,
                wanted,
                materializations,
                opened,
                partDescriptor,
                reopenedPartDescriptor,
            )
        } catch (error: Throwable) {
            opened.forEach(ParcelFileDescriptor::close)
            partDescriptor?.close()
            reopenedPartDescriptor?.close()
            staging?.let { delete(context, it) }
            part?.let { delete(context, it) }
            releaseGrant(context, treeUri)
            throw error
        }
    }

    fun publishAndPersist(
        context: Context,
        run: PreparedSafRun,
        report: EngineReport,
    ): JSONObject {
        check(report.preparedFiles.isNotEmpty()) {
            "native preparation returned no file hashes"
        }
        val resolver = context.contentResolver
        for (file in run.plan.files.filter { it.materialize }) {
            val temporary =
                requireNotNull(run.materializationUris[file.fileIndex]) {
                    "materialization URI is absent"
                }
            val finalName = file.path.last()
            check(findChild(context, parentOf(context, temporary), finalName) == null) {
                "materialization final document already exists"
            }
            requireNotNull(
                DocumentsContract.renameDocument(resolver, temporary, finalName),
            ) { "provider refused materialization publication" }
        }
        val grantRoot = documentUri(run.treeUri)
        check(findChild(context, grantRoot, run.plan.name) == null) {
            "late final SAF output collision"
        }
        val published =
            requireNotNull(
                DocumentsContract.renameDocument(
                    resolver,
                    run.stagingUri,
                    run.plan.name,
                ),
            ) { "provider refused final SAF publication" }
        val manifest = JSONArray()
        for (prepared in report.preparedFiles) {
            val planFile =
                run.plan.files.single { it.fileIndex == prepared.fileIndex }
            manifest.put(
                JSONObject()
                    .put("file_index", prepared.fileIndex.toLong())
                    .put("path", JSONArray(planFile.path))
                    .put("length", prepared.length.toLong())
                    .put("sha1", prepared.sha1Hex),
            )
        }
        val state =
            JSONObject()
                .put("run_id", "")
                .put("tree_uri", run.treeUri.toString())
                .put("published_uri", published.toString())
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
            .put("published_uri", published.toString())
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
        val published = Uri.parse(state.getString("published_uri"))
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
            val uri = requirePath(context, published, components)
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
        check(delete(context, published)) { "could not delete published SAF output" }
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
            .put("published_deleted", true)
            .put("part_deleted", true)
    }

    fun cleanup(
        context: Context,
        run: PreparedSafRun,
    ) {
        delete(context, run.stagingUri)
        delete(context, run.partUri)
        releaseGrant(context, run.treeUri)
    }

    private fun createPath(
        context: Context,
        root: Uri,
        components: List<String>,
        temporary: Boolean,
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
        val finalName =
            if (temporary) {
                ".${components.last()}.rstorrent-materializing"
            } else {
                components.last()
            }
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
                    "published SAF path is absent: ${components.joinToString("/")}"
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

    private fun parentOf(
        context: Context,
        child: Uri,
    ): Uri {
        val documentId = DocumentsContract.getDocumentId(child)
        val separator = documentId.lastIndexOf('/')
        check(separator > 0) { "provider document has no parent" }
        return DocumentsContract.buildDocumentUriUsingTree(
            child,
            documentId.substring(0, separator),
        )
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
