import { message as localizedMessage } from "../localization/runtime";
export const MAX_TORRENT_FILE_BYTES = 64 * 1024 * 1024;

export interface TorrentFileSource {
  readonly size: number;
  arrayBuffer(): Promise<ArrayBuffer>;
}

export function torrentFileSizeError(size: number): string | null {
  if (!Number.isSafeInteger(size) || size <= 0) {
    return localizedMessage("inspection.torrent.file.torrent.files.must.contain.at.least.one");
  }
  if (size > MAX_TORRENT_FILE_BYTES) {
    return localizedMessage("inspection.torrent.file.torrent.files.are.limited.to.64.mib");
  }
  return null;
}

export async function readTorrentFile(
  file: TorrentFileSource,
): Promise<ArrayBuffer> {
  const sizeError = torrentFileSizeError(file.size);
  if (sizeError !== null) throw new Error(sizeError);
  let source: ArrayBuffer;
  try {
    source = await file.arrayBuffer();
  } catch (error) {
    throw new Error(
      `Could not read the torrent file: ${
        error instanceof Error ? error.message : String(error)
      }`,
    );
  }
  if (source.byteLength !== file.size) {
    throw new Error("The torrent file changed while it was being read.");
  }
  return source;
}
