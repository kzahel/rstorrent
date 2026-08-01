export const MAX_TORRENT_INPUT_BYTES = 16_384;

export type ValidatedTorrentInput =
  | { readonly accepted: true; readonly magnet: string }
  | { readonly accepted: false; readonly message: string };

export function validateTorrentInput(source: string): ValidatedTorrentInput {
  const input = source.trim();
  if (input.length === 0) {
    return {
      accepted: false,
      message:
        "Paste a magnet link to add a torrent. .torrent file selection is not available yet.",
    };
  }
  if (/^https?:\/\//i.test(input)) {
    return {
      accepted: false,
      message:
        "Remote .torrent URLs are not supported yet. Paste a magnet link instead.",
    };
  }
  if (/^file:\/\//i.test(input)) {
    return {
      accepted: false,
      message:
        ".torrent file selection is not available yet. Paste a magnet link instead.",
    };
  }
  if (!input.startsWith("magnet:?")) {
    return {
      accepted: false,
      message: "Enter a magnet link beginning with magnet:?",
    };
  }
  if (new TextEncoder().encode(input).byteLength > MAX_TORRENT_INPUT_BYTES) {
    return {
      accepted: false,
      message: "Magnet links are limited to 16,384 bytes.",
    };
  }
  return { accepted: true, magnet: input };
}
