package org.rstorrent.bootstrap;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.CancellationSignal;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;
import android.util.Base64;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.atomic.AtomicInteger;

public final class ExternalIntakeFixtureProvider extends ContentProvider {
    public static final String VALID = "valid";
    public static final String EMPTY = "empty";
    public static final String OVERSIZED = "oversized";
    public static final String OVERSIZED_STREAM = "oversized-stream";
    public static final String NEAR_LIMIT = "near-limit";
    public static final String DENIED = "denied";
    public static final String DELAYED_ONCE = "delayed-once";
    public static final String FAILING = "failing";
    public static final String DIRECTORY = "directory";
    public static final String GENERIC_NAME = "generic";
    public static final String GENERIC_REJECTED = "generic-rejected";

    private static final int MAX_TORRENT_SOURCE_BYTES = 64 * 1024 * 1024;
    private static final byte[] DEFAULT_TORRENT = defaultTorrent();
    private static final AtomicInteger DELAYED_OPENS = new AtomicInteger();
    private static volatile byte[] configuredPayload = DEFAULT_TORRENT;

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public String getType(Uri uri) {
        switch (fixtureCase(uri)) {
            case DIRECTORY:
                return DocumentsContract.Document.MIME_TYPE_DIR;
            case GENERIC_NAME:
            case GENERIC_REJECTED:
                return "application/octet-stream";
            default:
                return "application/x-bittorrent";
        }
    }

    @Override
    public Cursor query(
            Uri uri,
            String[] projection,
            String selection,
            String[] selectionArgs,
            String sortOrder) {
        return queryResult(uri, projection);
    }

    @Override
    public Cursor query(
            Uri uri,
            String[] projection,
            String selection,
            String[] selectionArgs,
            String sortOrder,
            CancellationSignal cancellationSignal) {
        return queryResult(uri, projection);
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        return openFixture(uri, null);
    }

    @Override
    public ParcelFileDescriptor openFile(
            Uri uri, String mode, CancellationSignal signal) throws FileNotFoundException {
        return openFixture(uri, signal);
    }

    @Override
    public Bundle call(String method, String arg, Bundle extras) {
        if ("configure".equals(method)) {
            String payload = extras == null ? null : extras.getString("payload_base64");
            configuredPayload =
                    payload == null ? DEFAULT_TORRENT : Base64.decode(payload, Base64.DEFAULT);
            DELAYED_OPENS.set(0);
            return new Bundle();
        }
        if ("reset".equals(method)) {
            configuredPayload = DEFAULT_TORRENT;
            DELAYED_OPENS.set(0);
            return new Bundle();
        }
        return super.call(method, arg, extras);
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        throw new UnsupportedOperationException();
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException();
    }

    @Override
    public int update(
            Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        throw new UnsupportedOperationException();
    }

    private Cursor queryResult(Uri uri, String[] requestedProjection) {
        String[] projection =
                requestedProjection == null
                        ? new String[] {OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE}
                        : requestedProjection;
        MatrixCursor cursor = new MatrixCursor(projection);
        MatrixCursor.RowBuilder row = cursor.newRow();
        String fixture = fixtureCase(uri);
        for (String column : projection) {
            if (OpenableColumns.DISPLAY_NAME.equals(column)) {
                switch (fixture) {
                    case GENERIC_NAME:
                        row.add("generic-fixture.torrent");
                        break;
                    case GENERIC_REJECTED:
                        row.add("generic-fixture.bin");
                        break;
                    case DIRECTORY:
                        row.add("fixture-directory");
                        break;
                    default:
                        row.add("fixture.torrent");
                        break;
                }
            } else if (OpenableColumns.SIZE.equals(column)) {
                if (EMPTY.equals(fixture)) {
                    row.add(0L);
                } else if (OVERSIZED.equals(fixture)) {
                    row.add((long) MAX_TORRENT_SOURCE_BYTES + 1L);
                } else if (OVERSIZED_STREAM.equals(fixture) || NEAR_LIMIT.equals(fixture)) {
                    row.add(null);
                } else {
                    row.add((long) configuredPayload.length);
                }
            } else {
                row.add(null);
            }
        }
        return cursor;
    }

    private ParcelFileDescriptor openFixture(Uri uri, CancellationSignal signal)
            throws FileNotFoundException {
        String fixture = fixtureCase(uri);
        if (DENIED.equals(fixture)) {
            throw new SecurityException("fixture denial");
        }
        if (FAILING.equals(fixture)) {
            throw new FileNotFoundException("fixture failure");
        }
        final ParcelFileDescriptor[] pipe;
        try {
            pipe = ParcelFileDescriptor.createPipe();
        } catch (IOException error) {
            throw new FileNotFoundException("fixture pipe unavailable");
        }
        ParcelFileDescriptor read = pipe[0];
        ParcelFileDescriptor write = pipe[1];
        Thread writer =
                new Thread(
                        () -> {
                            try (OutputStream output =
                                    new ParcelFileDescriptor.AutoCloseOutputStream(write)) {
                                if (EMPTY.equals(fixture)) {
                                    return;
                                }
                                if (OVERSIZED.equals(fixture)
                                        || OVERSIZED_STREAM.equals(fixture)) {
                                    byte[] buffer = new byte[16 * 1024];
                                    long remaining = (long) MAX_TORRENT_SOURCE_BYTES + 1L;
                                    while (remaining > 0L) {
                                        int count = (int) Math.min(buffer.length, remaining);
                                        output.write(buffer, 0, count);
                                        remaining -= count;
                                    }
                                    return;
                                }
                                if (NEAR_LIMIT.equals(fixture)) {
                                    byte[] buffer = new byte[16 * 1024];
                                    long remaining = MAX_TORRENT_SOURCE_BYTES;
                                    while (remaining > 0L) {
                                        int count = (int) Math.min(buffer.length, remaining);
                                        output.write(buffer, 0, count);
                                        remaining -= count;
                                        Thread.sleep(1L);
                                    }
                                    return;
                                }
                                if (DELAYED_ONCE.equals(fixture)
                                        && DELAYED_OPENS.getAndIncrement() == 0) {
                                    Thread.sleep(31_000L);
                                }
                                output.write(configuredPayload);
                            } catch (InterruptedException error) {
                                Thread.currentThread().interrupt();
                                closeQuietly(write);
                            } catch (IOException ignored) {
                                // The target closed or cancelled its read side.
                            }
                        },
                        "external-intake-fixture-" + fixture);
        writer.setDaemon(true);
        if (signal != null) {
            signal.setOnCancelListener(
                    () -> {
                        writer.interrupt();
                        closeQuietly(write);
                    });
        }
        writer.start();
        return read;
    }

    private String fixtureCase(Uri uri) {
        String segment = uri.getLastPathSegment();
        return segment == null ? VALID : segment;
    }

    private static void closeQuietly(ParcelFileDescriptor descriptor) {
        try {
            descriptor.close();
        } catch (IOException ignored) {
            // Already closed by the pipe stream.
        }
    }

    private static byte[] defaultTorrent() {
        byte[] prefix =
                "d4:infod6:lengthi1e4:name7:fixture12:piece lengthi16384e6:pieces20:"
                        .getBytes(StandardCharsets.UTF_8);
        byte[] suffix = "ee".getBytes(StandardCharsets.UTF_8);
        byte[] result = new byte[prefix.length + 20 + suffix.length];
        System.arraycopy(prefix, 0, result, 0, prefix.length);
        System.arraycopy(suffix, 0, result, prefix.length + 20, suffix.length);
        return result;
    }
}
