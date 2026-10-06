/**
 * Bytes of a subtitle or text file → string. Plenty of Spanish .srt files are
 * Windows-1252 (Latin-1) rather than UTF-8, and `File.text()` would turn every
 * ñ and á in them into "�". Byte-order marks decide first (some Windows tools
 * write UTF-16), then strict UTF-8, then Windows-1252, which can decode any
 * byte sequence.
 */
export function decodeText(bytes: Uint8Array): string {
  if (bytes[0] === 0xff && bytes[1] === 0xfe) return new TextDecoder("utf-16le").decode(bytes);
  if (bytes[0] === 0xfe && bytes[1] === 0xff) return new TextDecoder("utf-16be").decode(bytes);
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return new TextDecoder("windows-1252").decode(bytes);
  }
}
