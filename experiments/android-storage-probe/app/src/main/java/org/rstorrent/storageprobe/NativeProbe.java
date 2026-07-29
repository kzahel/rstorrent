package org.rstorrent.storageprobe;

final class NativeProbe {
    static {
        System.loadLibrary("rstorrent_android_storage_probe");
    }

    private NativeProbe() {}

    static native long runSparse(int fd, long logicalLength);
    static native long truncateSparse(int fd, long logicalLength);
    static native long writeSparseMarkers(int fd, long logicalLength);
    static native long syncDescriptor(int fd);
    static native long verifySparse(int fd, long logicalLength);
    static native long writeMaterialized(int fd);
    static native long verifyMaterialized(int fd);
    static native int duplicate(int fd);
    static native long verifyOwned(int fd, long logicalLength);
    static native int closeOwned(int fd);
    static native long logicalBytes(int fd);
    static native long allocatedBytes(int fd);
    static native long filesystemType(int fd);
    static native long filesystemBlockBytes(int fd);
    static native int startCancellable(int fd, long maximumBytes);
    static native long cancellableProgress();
    static native long cancelAndJoin();
}
