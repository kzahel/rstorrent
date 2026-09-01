package org.rstorrent.bootstrap

import android.content.Context
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.File
import java.nio.ByteBuffer
import java.nio.file.Files
import java.util.Base64
import java.util.Comparator
import java.util.UUID
import java.util.stream.Collectors
import java.util.zip.CRC32

enum class ProductDataResetPhase {
    REMOVING_TORRENTS,
    RESETTING_PROFILE,
    RELEASING_ROOTS,
    RESETTING_PREFERENCES,
    RESTARTING_APPLICATION,
    VERIFYING_APPLICATION,
}

internal data class ProductDataResetFailure(
    val code: String,
    val detail: String,
    val torrentId: String? = null,
)

internal data class ProductDataResetJournal(
    val operationId: String,
    val deleteDataRequested: Boolean,
    val deleteRemainingData: Boolean = deleteDataRequested,
    val torrentIds: List<String>,
    val roots: List<ProductSafRootGrant>,
    val phase: ProductDataResetPhase = ProductDataResetPhase.REMOVING_TORRENTS,
    val nextTorrentIndex: Int = 0,
    val nextRootIndex: Int = 0,
    val failure: ProductDataResetFailure? = null,
) {
    val downgradedToKeep: Boolean
        get() = deleteDataRequested && !deleteRemainingData

    val completedTorrentCount: Int
        get() = nextTorrentIndex

    fun validate() {
        require(operationId.length <= MAX_OPERATION_ID_BYTES) { "data reset operation ID is too long" }
        require(UUID.fromString(operationId).toString() == operationId) {
            "data reset operation ID is not canonical"
        }
        require(torrentIds.size <= MAX_TORRENTS) { "data reset torrent count exceeds $MAX_TORRENTS" }
        require(torrentIds.distinct().size == torrentIds.size) { "data reset torrent IDs are duplicated" }
        require(torrentIds.all(TORRENT_ID::matches)) { "data reset contains an invalid torrent ID" }
        require(roots.size <= ProductSafRootRegistry.MAX_ROOTS) {
            "data reset root count exceeds ${ProductSafRootRegistry.MAX_ROOTS}"
        }
        ProductSafRootRegistry.validate(ProductSafRootRegistryState(roots = roots))
        require(nextTorrentIndex in 0..torrentIds.size) { "data reset torrent cursor is invalid" }
        require(nextRootIndex in 0..roots.size) { "data reset root cursor is invalid" }
        if (phase != ProductDataResetPhase.REMOVING_TORRENTS) {
            require(nextTorrentIndex == torrentIds.size) {
                "data reset advanced before every torrent completed"
            }
        }
        when (phase) {
            ProductDataResetPhase.REMOVING_TORRENTS,
            ProductDataResetPhase.RESETTING_PROFILE,
            -> require(nextRootIndex == 0) {
                "data reset released a root before profile reset"
            }
            ProductDataResetPhase.RELEASING_ROOTS -> Unit
            ProductDataResetPhase.RESETTING_PREFERENCES,
            ProductDataResetPhase.RESTARTING_APPLICATION,
            ProductDataResetPhase.VERIFYING_APPLICATION,
            -> require(nextRootIndex == roots.size) {
                "data reset advanced before every root grant completed"
            }
        }
        require(deleteDataRequested || !deleteRemainingData) {
            "keep-mode data reset cannot enable deletion"
        }
        failure?.let { value ->
            require(value.code.matches(FAILURE_CODE)) { "data reset failure code is invalid" }
            require(value.detail.toByteArray(Charsets.UTF_8).size <= MAX_FAILURE_DETAIL_BYTES) {
                "data reset failure detail exceeds $MAX_FAILURE_DETAIL_BYTES bytes"
            }
            value.torrentId?.let {
                require(it.matches(TORRENT_ID)) { "data reset failure torrent ID is invalid" }
                require(
                    phase == ProductDataResetPhase.REMOVING_TORRENTS &&
                        torrentIds.getOrNull(nextTorrentIndex) == it,
                ) { "data reset failure does not identify the current torrent" }
            }
        }
    }

    companion object {
        const val MAX_TORRENTS = 500
        const val MAX_ENCODED_BYTES = 512 * 1024
        const val MAX_FAILURE_DETAIL_BYTES = 512
        private const val MAX_OPERATION_ID_BYTES = 128
        private val TORRENT_ID = Regex("^t1-[0-9a-f]{32}$")
        private val FAILURE_CODE = Regex("^[a-z0-9_]{1,64}$")

        fun capture(
            deleteData: Boolean,
            torrentIds: List<String>,
            roots: List<ProductSafRootGrant>,
            operationId: String = UUID.randomUUID().toString(),
        ): ProductDataResetJournal =
            ProductDataResetJournal(
                operationId = operationId,
                deleteDataRequested = deleteData,
                torrentIds = torrentIds,
                roots = roots,
            ).also(ProductDataResetJournal::validate)
    }
}

internal object ProductDataResetJournalCodec {
    private const val VERSION = 1
    private const val CHECKSUM_BYTES = Long.SIZE_BYTES

    fun encode(journal: ProductDataResetJournal): String {
        journal.validate()
        val payloadBuffer = ByteArrayOutputStream()
        DataOutputStream(payloadBuffer).use { output ->
            output.writeInt(VERSION)
            output.writeUTF(journal.operationId)
            output.writeBoolean(journal.deleteDataRequested)
            output.writeBoolean(journal.deleteRemainingData)
            output.writeUTF(journal.phase.name)
            output.writeInt(journal.nextTorrentIndex)
            output.writeInt(journal.nextRootIndex)
            output.writeInt(journal.torrentIds.size)
            journal.torrentIds.forEach(output::writeUTF)
            output.writeInt(journal.roots.size)
            journal.roots.forEach { root ->
                output.writeUTF(root.rootId)
                output.writeUTF(root.label)
                output.writeUTF(root.treeUri)
                output.writeLong(root.generation)
            }
            output.writeBoolean(journal.failure != null)
            journal.failure?.let { failure ->
                output.writeUTF(failure.code)
                output.writeUTF(failure.detail)
                output.writeBoolean(failure.torrentId != null)
                failure.torrentId?.let(output::writeUTF)
            }
        }
        val payload = payloadBuffer.toByteArray()
        val checksum = CRC32().apply { update(payload) }.value
        val encodedBytes = ByteArrayOutputStream(payload.size + CHECKSUM_BYTES)
        encodedBytes.write(payload)
        DataOutputStream(encodedBytes).use { it.writeLong(checksum) }
        val encoded = Base64.getUrlEncoder().withoutPadding().encodeToString(encodedBytes.toByteArray())
        require(encoded.toByteArray(Charsets.US_ASCII).size <= ProductDataResetJournal.MAX_ENCODED_BYTES) {
            "data reset journal exceeds its encoded bound"
        }
        return encoded
    }

    fun decode(encoded: String): ProductDataResetJournal {
        require(encoded.toByteArray(Charsets.US_ASCII).size <= ProductDataResetJournal.MAX_ENCODED_BYTES) {
            "data reset journal exceeds its encoded bound"
        }
        val bytes = Base64.getUrlDecoder().decode(encoded)
        require(bytes.size in (CHECKSUM_BYTES + 1)..ProductDataResetJournal.MAX_ENCODED_BYTES) {
            "data reset journal has an invalid decoded size"
        }
        val payload = bytes.copyOf(bytes.size - CHECKSUM_BYTES)
        val expectedChecksum = ByteBuffer.wrap(bytes, payload.size, CHECKSUM_BYTES).long
        val actualChecksum = CRC32().apply { update(payload) }.value
        require(expectedChecksum == actualChecksum) { "data reset journal checksum differs" }
        val journal =
            DataInputStream(ByteArrayInputStream(payload)).use { input ->
                require(input.readInt() == VERSION) { "unsupported data reset journal version" }
                val operationId = input.readUTF()
                val deleteRequested = input.readBoolean()
                val deleteRemaining = input.readBoolean()
                val phase = ProductDataResetPhase.valueOf(input.readUTF())
                val nextTorrentIndex = input.readInt()
                val nextRootIndex = input.readInt()
                val torrentCount = input.readInt()
                require(torrentCount in 0..ProductDataResetJournal.MAX_TORRENTS) {
                    "invalid data reset torrent count"
                }
                val torrents = List(torrentCount) { input.readUTF() }
                val rootCount = input.readInt()
                require(rootCount in 0..ProductSafRootRegistry.MAX_ROOTS) {
                    "invalid data reset root count"
                }
                val roots =
                    List(rootCount) {
                        ProductSafRootGrant(
                            rootId = input.readUTF(),
                            label = input.readUTF(),
                            treeUri = input.readUTF(),
                            generation = input.readLong(),
                        )
                    }
                val failure =
                    if (input.readBoolean()) {
                        ProductDataResetFailure(
                            code = input.readUTF(),
                            detail = input.readUTF(),
                            torrentId = if (input.readBoolean()) input.readUTF() else null,
                        )
                    } else {
                        null
                    }
                require(input.read() == -1) { "data reset journal contains trailing data" }
                ProductDataResetJournal(
                    operationId = operationId,
                    deleteDataRequested = deleteRequested,
                    deleteRemainingData = deleteRemaining,
                    torrentIds = torrents,
                    roots = roots,
                    phase = phase,
                    nextTorrentIndex = nextTorrentIndex,
                    nextRootIndex = nextRootIndex,
                    failure = failure,
                )
            }
        journal.validate()
        return journal
    }
}

internal object ProductDataResetJournalStore {
    private const val PREFERENCES = "product_data_reset"
    private const val JOURNAL = "journal_v1"

    @Synchronized
    fun load(context: Context): ProductDataResetJournal? =
        context
            .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(JOURNAL, null)
            ?.let(ProductDataResetJournalCodec::decode)

    @Synchronized
    fun persist(
        context: Context,
        journal: ProductDataResetJournal,
    ) {
        check(
            context
                .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .putString(JOURNAL, ProductDataResetJournalCodec.encode(journal))
                .commit(),
        ) { "could not synchronously persist the data reset journal" }
    }

    @Synchronized
    fun clear(context: Context) {
        check(
            context
                .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .remove(JOURNAL)
                .commit(),
        ) { "could not synchronously clear the data reset journal" }
    }
}

internal object ProductPrivateProfileReset {
    const val PROFILE_DIRECTORY = "product-profile"
    private const val MAX_PROFILE_ENTRIES = 4_096

    fun reset(
        filesDirectory: File,
        profile: File = File(filesDirectory, PROFILE_DIRECTORY),
    ) {
        val filesCanonical = filesDirectory.canonicalFile
        val profileAbsolute = profile.absoluteFile
        require(profileAbsolute.name == PROFILE_DIRECTORY) { "unexpected product profile name" }
        require(profileAbsolute.parentFile?.canonicalFile == filesCanonical) {
            "product profile is outside the application files directory"
        }
        val path = profileAbsolute.toPath()
        require(!Files.isSymbolicLink(path)) { "product profile is a symbolic link" }
        if (Files.exists(path)) {
            val entries =
                Files.walk(path).use { stream ->
                    stream
                        .limit((MAX_PROFILE_ENTRIES + 1).toLong())
                        .collect(Collectors.toList())
                }
            require(entries.size <= MAX_PROFILE_ENTRIES) {
                "product profile exceeds its bounded reset entry count"
            }
            entries.forEach { entry ->
                require(entry.normalize().startsWith(path.normalize())) {
                    "product profile entry escaped its fixed root"
                }
                require(!Files.isSymbolicLink(entry)) {
                    "product profile contains a symbolic link"
                }
            }
            entries.sortedWith(Comparator.reverseOrder()).forEach(Files::deleteIfExists)
        }
        check(profileAbsolute.mkdir() || profileAbsolute.isDirectory) {
            "could not recreate the product profile"
        }
    }
}
