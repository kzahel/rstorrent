package org.rstorrent.storageprobe;

import android.app.Activity;
import android.app.ActivityManager;
import android.content.ContentResolver;
import android.content.Intent;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Debug;
import android.os.ParcelFileDescriptor;
import android.os.SystemClock;
import android.provider.DocumentsContract;
import android.system.Os;
import android.widget.TextView;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class MainActivity extends Activity {
    private static final int TREE_REQUEST = 41;
    private static final int BLOCK_LENGTH = 16 * 1024;
    private static final long SPARSE_OFFSET = 256L * 1024L * 1024L;
    private static final long LOGICAL_LENGTH = SPARSE_OFFSET + BLOCK_LENGTH;
    private static final long CANCELLATION_MAXIMUM = 64L * 1024L * 1024L;
    private static final String PREFERENCES = "probe";
    private static final String TREE_URI = "tree_uri";
    private static final String PROBE_ROOT_URI = "probe_root_uri";
    private static final String SPARSE_URI = "sparse_uri";
    private static final String MATERIALIZED_URI = "materialized_uri";
    private static final String PRIVATE_FILE = "private-sparse.bin";
    private static final String RESULT_FILE = "result.json";
    private static final String PROBE_PREFIX = "rstorrent-storage-probe-";

    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private TextView status;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        status = new TextView(this);
        status.setPadding(32, 32, 32, 32);
        status.setTextSize(15);
        status.setText("RSTorrent storage probe starting");
        setContentView(status);

        String mode = getIntent().getStringExtra("mode");
        if ("verify".equals(mode)) {
            runInBackground(this::runRestartVerification);
        } else if ("cleanup".equals(mode)) {
            runInBackground(this::runCleanupOnly);
        } else {
            acquireTree();
        }
    }

    @Override
    protected void onDestroy() {
        executor.shutdownNow();
        super.onDestroy();
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != TREE_REQUEST) {
            return;
        }
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            writeFailure("initial", new IllegalStateException("document tree grant was cancelled"));
            return;
        }
        Uri treeUri = data.getData();
        int requiredFlags = Intent.FLAG_GRANT_READ_URI_PERMISSION
                | Intent.FLAG_GRANT_WRITE_URI_PERMISSION;
        try {
            if ((data.getFlags() & requiredFlags) != requiredFlags) {
                throw new IllegalStateException(
                        "document tree grant did not include read and write access"
                );
            }
            getContentResolver().takePersistableUriPermission(
                    treeUri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION
                            | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            );
            requirePreferenceCommit(
                    preferences().edit().putString(TREE_URI, treeUri.toString()),
                    "persist tree URI"
            );
            runInBackground(() -> runInitial(treeUri));
        } catch (Throwable error) {
            writeFailure("initial", error);
        }
    }

    private void acquireTree() {
        updateStatus("Choose the probe directory, then approve “Use this folder”.");
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        intent.addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
        intent.addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        intent.addFlags(Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
        String requestedInitialUri = getIntent().getStringExtra("tree_initial_uri");
        Uri initialUri = requestedInitialUri == null
                ? Uri.parse(
                        "content://com.android.externalstorage.documents/"
                                + "document/primary%3ADownload"
                )
                : Uri.parse(requestedInitialUri);
        intent.putExtra("android.provider.extra.INITIAL_URI", initialUri);
        startActivityForResult(intent, TREE_REQUEST);
    }

    private void runInitial(Uri treeUri) {
        JSONObject result = baseResult("initial");
        Uri probeRoot = null;
        try {
            result.put("tree_uri", treeUri.toString());
            snapshotMemory(result, "memory_before");
            String runId = Long.toString(System.currentTimeMillis());
            probeRoot = requireUri(DocumentsContract.createDocument(
                    getContentResolver(),
                    documentUri(treeUri),
                    DocumentsContract.Document.MIME_TYPE_DIR,
                    PROBE_PREFIX + "staging-" + runId
            ), "create probe staging directory");

            JSONObject provider = queryDocument(probeRoot);
            provider.put("authority", treeUri.getAuthority());
            provider.put("tree_uri", treeUri.toString());
            provider.put("persisted_permission_count",
                    getContentResolver().getPersistedUriPermissions().size());
            result.put("provider", provider);

            File privateFile = new File(getFilesDir(), PRIVATE_FILE);
            deleteIfPresent(privateFile);
            JSONObject privateResult = probeDescriptorBackend(
                    "app_private",
                    () -> ParcelFileDescriptor.open(
                            privateFile,
                            ParcelFileDescriptor.MODE_CREATE
                                    | ParcelFileDescriptor.MODE_READ_WRITE
                    )
            );
            result.put("app_private", privateResult);

            Uri sparseUri = requireUri(DocumentsContract.createDocument(
                    getContentResolver(),
                    probeRoot,
                    "application/octet-stream",
                    "sparse.bin"
            ), "create sparse SAF document");
            Uri initialSparseUri = sparseUri;
            JSONObject safResult = probeDescriptorBackend(
                    "saf",
                    () -> requireDescriptor(
                            getContentResolver().openFileDescriptor(initialSparseUri, "rw"),
                            "open sparse SAF document"
                    )
            );
            result.put("saf", safResult);

            Uri cancellationUri = requireUri(DocumentsContract.createDocument(
                    getContentResolver(),
                    probeRoot,
                    "application/octet-stream",
                    "cancellation.bin"
            ), "create cancellation document");
            result.put("cancellation", runCancellation(cancellationUri));

            JSONObject directoryRename = new JSONObject();
            Uri publishedRoot = renameCapability(
                    probeRoot,
                    PROBE_PREFIX + "published-" + runId,
                    directoryRename
            );
            result.put("directory_rename", directoryRename);
            probeRoot = publishedRoot;

            sparseUri = requireUri(
                    findChild(probeRoot, "sparse.bin"),
                    "find sparse document after directory rename"
            );

            JSONObject materialization = runMaterialization(probeRoot);
            result.put("materialization", materialization);
            Uri materializedUri = Uri.parse(materialization.getString("final_uri"));

            Uri cancellationAfterRename = findChild(probeRoot, "cancellation.bin");
            if (cancellationAfterRename != null) {
                DocumentsContract.deleteDocument(
                        getContentResolver(),
                        cancellationAfterRename
                );
            }
            requirePreferenceCommit(
                    preferences().edit()
                            .putString(PROBE_ROOT_URI, probeRoot.toString())
                            .putString(SPARSE_URI, sparseUri.toString())
                            .putString(MATERIALIZED_URI, materializedUri.toString()),
                    "persist restart state"
            );

            snapshotMemory(result, "memory_after");
            result.put("descriptor_count_after", descriptorCount());
            result.put("ready_for_restart", true);
            result.put("success",
                    privateResult.getLong("error_mask") == 0
                            && safResult.getLong("error_mask") == 0
                            && result.getJSONObject("cancellation").getBoolean("success")
                            && materialization.getBoolean("success")
                            && !"failed".equals(directoryRename.getString("state")));
            writeResult(result);
        } catch (Throwable error) {
            if (probeRoot != null) {
                preferences().edit()
                        .putString(PROBE_ROOT_URI, probeRoot.toString())
                        .commit();
            }
            writeFailure("initial", error, result);
        }
    }

    private void runRestartVerification() {
        JSONObject result = baseResult("restart");
        try {
            SharedPreferences preferences = preferences();
            Uri treeUri = preferenceUri(preferences, TREE_URI);
            Uri probeRoot = preferenceUri(preferences, PROBE_ROOT_URI);
            Uri sparseUri = preferenceUri(preferences, SPARSE_URI);
            Uri materializedUri = preferenceUri(preferences, MATERIALIZED_URI);
            result.put("persisted_permission_count",
                    getContentResolver().getPersistedUriPermissions().size());
            result.put("tree_uri", treeUri.toString());
            snapshotMemory(result, "memory_before");

            File privateFile = new File(getFilesDir(), PRIVATE_FILE);
            result.put("app_private_restart", verifyDescriptorBackend(
                    () -> ParcelFileDescriptor.open(
                            privateFile,
                            ParcelFileDescriptor.MODE_READ_WRITE
                    )
            ));
            result.put("saf_restart", verifyDescriptorBackend(
                    () -> requireDescriptor(
                            getContentResolver().openFileDescriptor(sparseUri, "rw"),
                            "reopen sparse SAF document after restart"
                    )
            ));
            result.put("materialized_restart", verifyMaterialized(materializedUri));

            boolean deleted = DocumentsContract.deleteDocument(
                    getContentResolver(),
                    probeRoot
            );
            deleteIfPresent(privateFile);
            getContentResolver().releasePersistableUriPermission(
                    treeUri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION
                            | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            );
            requirePreferenceCommit(
                    preferences.edit().clear(),
                    "clear completed probe state"
            );

            snapshotMemory(result, "memory_after");
            result.put("descriptor_count_after", descriptorCount());
            result.put("probe_tree_deleted", deleted);
            result.put("private_file_deleted", !privateFile.exists());
            result.put("persisted_permission_count_after",
                    getContentResolver().getPersistedUriPermissions().size());
            result.put("success",
                    result.getJSONObject("app_private_restart").getLong("error_mask") == 0
                            && result.getJSONObject("saf_restart").getLong("error_mask") == 0
                            && result.getJSONObject("materialized_restart")
                                    .getLong("error_mask") == 0
                            && deleted
                            && !privateFile.exists());
            writeResult(result);
        } catch (Throwable error) {
            writeFailure("restart", error, result);
        }
    }

    private void runCleanupOnly() {
        JSONObject result = baseResult("cleanup");
        SharedPreferences preferences = preferences();
        try {
            String rootValue = preferences.getString(PROBE_ROOT_URI, null);
            boolean deleted = true;
            if (rootValue != null) {
                deleted = DocumentsContract.deleteDocument(
                        getContentResolver(),
                        Uri.parse(rootValue)
                );
            }
            String treeValue = preferences.getString(TREE_URI, null);
            if (treeValue != null) {
                try {
                    getContentResolver().releasePersistableUriPermission(
                            Uri.parse(treeValue),
                            Intent.FLAG_GRANT_READ_URI_PERMISSION
                                    | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                    );
                } catch (SecurityException ignored) {
                    result.put("permission_was_already_absent", true);
                }
            }
            File privateFile = new File(getFilesDir(), PRIVATE_FILE);
            deleteIfPresent(privateFile);
            requirePreferenceCommit(
                    preferences.edit().clear(),
                    "clear recovery state"
            );
            result.put("probe_tree_deleted", deleted);
            result.put("private_file_deleted", !privateFile.exists());
            result.put("success", deleted && !privateFile.exists());
            writeResult(result);
        } catch (Throwable error) {
            writeFailure("cleanup", error, result);
        }
    }

    private JSONObject probeDescriptorBackend(
            String name,
            DescriptorOpener opener
    ) throws Exception {
        JSONObject result = new JSONObject();
        result.put("name", name);
        result.put("logical_target", LOGICAL_LENGTH);
        result.put("sparse_offset", SPARSE_OFFSET);
        result.put("block_length", BLOCK_LENGTH);
        result.put("descriptor_count_before", descriptorCount());

        ParcelFileDescriptor descriptor = opener.open();
        int borrowedFd = descriptor.getFd();
        putFilesystem(result, borrowedFd, "");
        result.put("logical_before", NativeProbe.logicalBytes(borrowedFd));
        result.put("allocated_before", NativeProbe.allocatedBytes(borrowedFd));
        long runStarted = SystemClock.elapsedRealtimeNanos();
        long stageStarted = runStarted;
        long errorMask = NativeProbe.truncateSparse(borrowedFd, LOGICAL_LENGTH);
        result.put("truncate_nanos", SystemClock.elapsedRealtimeNanos() - stageStarted);
        stageStarted = SystemClock.elapsedRealtimeNanos();
        errorMask |= NativeProbe.writeSparseMarkers(borrowedFd, LOGICAL_LENGTH);
        result.put("write_nanos", SystemClock.elapsedRealtimeNanos() - stageStarted);
        stageStarted = SystemClock.elapsedRealtimeNanos();
        errorMask |= NativeProbe.syncDescriptor(borrowedFd);
        result.put("sync_nanos", SystemClock.elapsedRealtimeNanos() - stageStarted);
        stageStarted = SystemClock.elapsedRealtimeNanos();
        errorMask |= NativeProbe.verifySparse(borrowedFd, LOGICAL_LENGTH);
        result.put("read_nanos", SystemClock.elapsedRealtimeNanos() - stageStarted);
        result.put("run_nanos", SystemClock.elapsedRealtimeNanos() - runStarted);
        result.put("error_mask", errorMask);
        result.put("logical_after", NativeProbe.logicalBytes(borrowedFd));
        result.put("allocated_after", NativeProbe.allocatedBytes(borrowedFd));

        Os.fstat(descriptor.getFileDescriptor());
        result.put("caller_descriptor_survived", true);
        int ownedFd = NativeProbe.duplicate(borrowedFd);
        if (ownedFd < 0) {
            descriptor.close();
            throw new IOException("native descriptor duplication failed");
        }
        descriptor.close();
        long ownedMask;
        try {
            ownedMask = NativeProbe.verifyOwned(ownedFd, LOGICAL_LENGTH);
        } finally {
            int closeResult = NativeProbe.closeOwned(ownedFd);
            result.put("owned_close_result", closeResult);
        }
        result.put("owned_after_java_close_mask", ownedMask);
        errorMask |= ownedMask;

        ParcelFileDescriptor reopened = opener.open();
        long reopenedStarted = SystemClock.elapsedRealtimeNanos();
        long reopenMask;
        try {
            putFilesystem(result, reopened.getFd(), "_reopened");
            reopenMask = NativeProbe.verifySparse(reopened.getFd(), LOGICAL_LENGTH);
            result.put("logical_reopened", NativeProbe.logicalBytes(reopened.getFd()));
            result.put("allocated_reopened", NativeProbe.allocatedBytes(reopened.getFd()));
        } finally {
            reopened.close();
        }
        result.put("reopen_nanos", SystemClock.elapsedRealtimeNanos() - reopenedStarted);
        result.put("reopen_error_mask", reopenMask);
        errorMask |= reopenMask;
        result.put("error_mask", errorMask);
        result.put("descriptor_count_after", descriptorCount());
        return result;
    }

    private JSONObject verifyDescriptorBackend(DescriptorOpener opener) throws Exception {
        JSONObject result = new JSONObject();
        ParcelFileDescriptor descriptor = opener.open();
        long started = SystemClock.elapsedRealtimeNanos();
        try {
            putFilesystem(result, descriptor.getFd(), "");
            result.put("error_mask",
                    NativeProbe.verifySparse(descriptor.getFd(), LOGICAL_LENGTH));
            result.put("logical_bytes", NativeProbe.logicalBytes(descriptor.getFd()));
            result.put("allocated_bytes", NativeProbe.allocatedBytes(descriptor.getFd()));
        } finally {
            descriptor.close();
        }
        result.put("verify_nanos", SystemClock.elapsedRealtimeNanos() - started);
        return result;
    }

    private JSONObject runCancellation(Uri cancellationUri) throws Exception {
        JSONObject result = new JSONObject();
        ParcelFileDescriptor descriptor = requireDescriptor(
                getContentResolver().openFileDescriptor(cancellationUri, "rw"),
                "open cancellation document"
        );
        long started = SystemClock.elapsedRealtimeNanos();
        int startResult = NativeProbe.startCancellable(
                descriptor.getFd(),
                CANCELLATION_MAXIMUM
        );
        descriptor.close();
        long progressStarted = SystemClock.elapsedRealtimeNanos();
        long progressDeadline = SystemClock.elapsedRealtime() + 2_000;
        long observedProgress = NativeProbe.cancellableProgress();
        while (observedProgress == 0
                && SystemClock.elapsedRealtime() < progressDeadline) {
            SystemClock.sleep(5);
            observedProgress = NativeProbe.cancellableProgress();
        }
        result.put(
                "progress_wait_nanos",
                SystemClock.elapsedRealtimeNanos() - progressStarted
        );
        result.put("progress_before_cancel", observedProgress);
        if (observedProgress > 0) {
            SystemClock.sleep(30);
        }
        long written = NativeProbe.cancelAndJoin();
        result.put("start_result", startResult);
        result.put("written_bytes", written);
        result.put("maximum_bytes", CANCELLATION_MAXIMUM);
        result.put("elapsed_nanos", SystemClock.elapsedRealtimeNanos() - started);
        result.put("terminated", true);
        result.put("success",
                startResult == 0
                        && observedProgress > 0
                        && written > 0
                        && written < CANCELLATION_MAXIMUM);
        return result;
    }

    private JSONObject runMaterialization(Uri parent) throws Exception {
        JSONObject result = new JSONObject();
        Uri temporary = requireUri(DocumentsContract.createDocument(
                getContentResolver(),
                parent,
                "application/octet-stream",
                ".materializing.bin"
        ), "create materialization temporary document");
        ParcelFileDescriptor descriptor = requireDescriptor(
                getContentResolver().openFileDescriptor(temporary, "rw"),
                "open materialization temporary document"
        );
        long writeMask;
        try {
            writeMask = NativeProbe.writeMaterialized(descriptor.getFd());
        } finally {
            descriptor.close();
        }
        long reopenMask = verifyMaterialized(temporary).getLong("error_mask");
        JSONObject rename = new JSONObject();
        Uri finalUri = renameCapability(temporary, "materialized.bin", rename);
        long finalMask = verifyMaterialized(finalUri).getLong("error_mask");
        result.put("write_error_mask", writeMask);
        result.put("reopen_error_mask", reopenMask);
        result.put("final_error_mask", finalMask);
        result.put("rename", rename);
        result.put("final_uri", finalUri.toString());
        result.put("success",
                writeMask == 0
                        && reopenMask == 0
                        && finalMask == 0
                        && !"failed".equals(rename.getString("state")));
        return result;
    }

    private JSONObject verifyMaterialized(Uri uri) throws Exception {
        JSONObject result = new JSONObject();
        ParcelFileDescriptor descriptor = requireDescriptor(
                getContentResolver().openFileDescriptor(uri, "rw"),
                "open materialized document"
        );
        try {
            result.put("error_mask",
                    NativeProbe.verifyMaterialized(descriptor.getFd()));
        } finally {
            descriptor.close();
        }
        return result;
    }

    private Uri renameCapability(
            Uri document,
            String displayName,
            JSONObject result
    ) throws JSONException {
        try {
            Uri renamed = DocumentsContract.renameDocument(
                    getContentResolver(),
                    document,
                    displayName
            );
            if (renamed == null) {
                result.put("state", "unsupported");
                result.put("uri", document.toString());
                return document;
            }
            result.put("state", "supported");
            result.put("uri", renamed.toString());
            return renamed;
        } catch (UnsupportedOperationException error) {
            result.put("state", "unsupported");
            result.put("detail", error.toString());
            result.put("uri", document.toString());
            return document;
        } catch (Throwable error) {
            result.put("state", "failed");
            result.put("detail", error.toString());
            result.put("uri", document.toString());
            return document;
        }
    }

    private Uri findChild(Uri parent, String displayName) throws Exception {
        for (Child child : listChildren(parent)) {
            if (displayName.equals(child.name)) {
                return child.uri;
            }
        }
        return null;
    }

    private List<Child> listChildren(Uri parent) throws Exception {
        String parentId = DocumentsContract.getDocumentId(parent);
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(parent, parentId);
        java.util.ArrayList<Child> children = new java.util.ArrayList<>();
        try (Cursor cursor = getContentResolver().query(
                childrenUri,
                new String[]{
                        DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                        DocumentsContract.Document.COLUMN_DISPLAY_NAME
                },
                null,
                null,
                null
        )) {
            if (cursor == null) {
                throw new IOException("document provider returned a null child cursor");
            }
            while (cursor.moveToNext()) {
                String id = cursor.getString(0);
                String name = cursor.getString(1);
                children.add(new Child(
                        DocumentsContract.buildDocumentUriUsingTree(parent, id),
                        name
                ));
            }
        }
        return children;
    }

    private JSONObject queryDocument(Uri document) throws Exception {
        JSONObject result = new JSONObject();
        try (Cursor cursor = getContentResolver().query(
                document,
                new String[]{
                        DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                        DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                        DocumentsContract.Document.COLUMN_MIME_TYPE,
                        DocumentsContract.Document.COLUMN_FLAGS
                },
                null,
                null,
                null
        )) {
            if (cursor == null || !cursor.moveToFirst()) {
                throw new IOException("document provider returned no document metadata");
            }
            result.put("document_id", cursor.getString(0));
            result.put("display_name", cursor.getString(1));
            result.put("mime_type", cursor.getString(2));
            result.put("flags", cursor.getLong(3));
        }
        return result;
    }

    private Uri documentUri(Uri treeUri) {
        return DocumentsContract.buildDocumentUriUsingTree(
                treeUri,
                DocumentsContract.getTreeDocumentId(treeUri)
        );
    }

    private JSONObject baseResult(String phase) {
        JSONObject result = new JSONObject();
        try {
            result.put("phase", phase);
            result.put("sdk", Build.VERSION.SDK_INT);
            result.put("model", Build.MODEL);
            result.put("device", Build.DEVICE);
            result.put("fingerprint", Build.FINGERPRINT);
            result.put("abis", new JSONArray(Arrays.asList(Build.SUPPORTED_ABIS)));
            String storage = getIntent().getStringExtra("storage");
            if (storage != null) {
                result.put("storage", storage);
            }
            result.put("logical_length", LOGICAL_LENGTH);
            result.put("sparse_offset", SPARSE_OFFSET);
            result.put("block_length", BLOCK_LENGTH);
            result.put("descriptor_count_before", descriptorCount());
        } catch (JSONException error) {
            throw new IllegalStateException(error);
        }
        return result;
    }

    private void snapshotMemory(JSONObject result, String key) throws JSONException {
        Debug.MemoryInfo memoryInfo = new Debug.MemoryInfo();
        Debug.getMemoryInfo(memoryInfo);
        JSONObject memory = new JSONObject();
        memory.put("total_pss_kib", memoryInfo.getTotalPss());
        memory.put("native_heap_allocated", Debug.getNativeHeapAllocatedSize());
        Runtime runtime = Runtime.getRuntime();
        memory.put("java_heap_used", runtime.totalMemory() - runtime.freeMemory());
        result.put(key, memory);
    }

    private int descriptorCount() {
        String[] descriptors = new File("/proc/self/fd").list();
        return descriptors == null ? -1 : descriptors.length;
    }

    private void putFilesystem(
            JSONObject result,
            int descriptor,
            String suffix
    ) throws JSONException {
        long type = NativeProbe.filesystemType(descriptor);
        result.put("filesystem_type" + suffix, type);
        if (type >= 0) {
            result.put("filesystem_type_hex" + suffix, Long.toHexString(type));
        }
        result.put(
                "filesystem_block_bytes" + suffix,
                NativeProbe.filesystemBlockBytes(descriptor)
        );
    }

    private Uri preferenceUri(SharedPreferences preferences, String key) {
        String value = preferences.getString(key, null);
        if (value == null) {
            throw new IllegalStateException("missing persisted " + key);
        }
        return Uri.parse(value);
    }

    private SharedPreferences preferences() {
        return getSharedPreferences(PREFERENCES, MODE_PRIVATE);
    }

    private void requirePreferenceCommit(
            SharedPreferences.Editor editor,
            String operation
    ) throws IOException {
        if (!editor.commit()) {
            throw new IOException(operation + " failed");
        }
    }

    private void runInBackground(ThrowingRunnable runnable) {
        executor.execute(() -> {
            try {
                runnable.run();
            } catch (Throwable error) {
                writeFailure("background", error);
            }
        });
    }

    private void writeFailure(String phase, Throwable error) {
        writeFailure(phase, error, baseResult(phase));
    }

    private void writeFailure(String phase, Throwable error, JSONObject result) {
        try {
            result.put("phase", phase);
            result.put("success", false);
            result.put("error", error.toString());
            StringWriter stack = new StringWriter();
            error.printStackTrace(new PrintWriter(stack));
            result.put("stack", stack.toString());
        } catch (JSONException jsonError) {
            throw new IllegalStateException(jsonError);
        }
        writeResult(result);
    }

    private void writeResult(JSONObject result) {
        try {
            File temporary = new File(getFilesDir(), RESULT_FILE + ".tmp");
            File destination = new File(getFilesDir(), RESULT_FILE);
            try (FileOutputStream output = new FileOutputStream(temporary)) {
                output.write(result.toString().getBytes(StandardCharsets.UTF_8));
                output.getFD().sync();
            }
            if (!temporary.renameTo(destination)) {
                throw new IOException("could not publish result file");
            }
            runOnUiThread(() -> updateStatus(result.toString()));
        } catch (Throwable writeError) {
            runOnUiThread(() -> updateStatus("Result write failed: " + writeError));
        }
    }

    private void updateStatus(String message) {
        status.setText(message);
    }

    private static Uri requireUri(Uri uri, String operation) throws IOException {
        if (uri == null) {
            throw new IOException(operation + " returned null");
        }
        return uri;
    }

    private static ParcelFileDescriptor requireDescriptor(
            ParcelFileDescriptor descriptor,
            String operation
    ) throws IOException {
        if (descriptor == null) {
            throw new IOException(operation + " returned null");
        }
        return descriptor;
    }

    private static void deleteIfPresent(File file) throws IOException {
        if (file.exists() && !file.delete()) {
            throw new IOException("could not delete " + file);
        }
    }

    @FunctionalInterface
    private interface DescriptorOpener {
        ParcelFileDescriptor open() throws Exception;
    }

    @FunctionalInterface
    private interface ThrowingRunnable {
        void run() throws Exception;
    }

    private static final class Child {
        final Uri uri;
        final String name;

        Child(Uri uri, String name) {
            this.uri = uri;
            this.name = name;
        }
    }
}
