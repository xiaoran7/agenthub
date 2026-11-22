package com.agentplatform.mobile.data

import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.StandardCharsets
import java.util.Base64

// =========================================================================
// Attachment encoding
//
// A picked file becomes a `RemoteAttachment`, which is four fields on the wire:
// `name`, `media_type`, `data`, `encoding`. The encoding choice is not free:
//
//   * `utf8` is for text. It keeps the payload small and the agent reads it
//     verbatim.
//   * `base64` is for everything else. Base64 grows the payload by 4/3, and the
//     contract caps `data` at 8MB *characters*, so a 6MB binary already
//     encodes past the limit.
//
// Both facts are why the reader refuses a file rather than letting the Gateway
// reject the whole prompt: a refusal the person can see is a choice, a 400
// after they pressed send is a bug they cannot act on.
//
// The text/binary decision cannot be made from the name and the MIME type
// alone. `text/plain` does not say which encoding the bytes are in, so a UTF-16
// or Latin-1 `.txt` passes both checks and arrives as mojibake — the failure the
// decision exists to prevent. The reader therefore sniffs the first 8KB as well;
// see [AttachmentReader.isText].
//
// `java.util.Base64` rather than `android.util.Base64`: the latter is a stub
// that throws in a JVM unit test, and this reader is pure logic worth testing
// directly. `java.util.Base64` needs API 26, which is already `minSdk`.
// =========================================================================

/** Extensions read as text. Mirrors the console's list so both agree. */
private val TEXT_EXTENSIONS =
    setOf(
        "txt", "md", "markdown", "json", "jsonl", "yaml", "yml", "toml", "xml", "csv", "tsv",
        "py", "ts", "tsx", "js", "jsx", "mjs", "cjs", "rs", "go", "java", "kt", "kts", "c", "cpp",
        "h", "hpp", "cs", "rb", "php", "swift", "sh", "bash", "zsh", "ps1", "bat", "cmd", "sql",
        "html", "css", "scss", "less", "vue", "svelte", "ini", "conf", "cfg", "env", "log",
    )

/** MIME prefixes read as text without looking at the extension. */
private val TEXT_MIME_PREFIXES = listOf("text/")

/**
 * MIME types that are obviously binary, whatever the file is called.
 *
 * The resolver decides these from the bytes, so they outrank the name. This set
 * is a *hint*: it can refuse a file, but it can no longer make one text — see
 * [AttachmentReader.isText], where the bytes carry the verdict and an unknown
 * application type with a recognised text extension still has to decode.
 *
 * The list is deliberately short. An unknown application type is left to the
 * extension and the bytes, because refusing to guess would send a `.kt` file the
 * resolver mislabelled as octet-stream down the base64 path for no reason.
 */
private val BINARY_MIME =
    setOf(
        "image/png",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/bmp",
        "image/tiff",
        "image/x-icon",
        "image/heic",
        "image/avif",
        "audio/mpeg",
        "audio/ogg",
        "audio/wav",
        "audio/flac",
        "audio/aac",
        "audio/webm",
        "video/mp4",
        "video/quicktime",
        "video/x-msvideo",
        "video/webm",
        "application/pdf",
        "application/zip",
        "application/gzip",
        "application/x-tar",
        "application/x-7z-compressed",
        "application/x-rar-compressed",
        "application/vnd.android.package-archive",
        "application/wasm",
    )

private val TEXT_MIME_EXACT =
    setOf(
        "application/json",
        "application/xml",
        "application/javascript",
        "application/x-yaml",
        "application/toml",
    )

/** What a picked file turned into, or why it could not. */
sealed interface AttachmentResult {
    data class Ready(val attachment: RemoteAttachment) : AttachmentResult

    /** A sentence the person can act on. */
    data class Rejected(val reason: String) : AttachmentResult
}

/** Reads one picked stream into a wire attachment. */
object AttachmentReader {
    /** Hard ceiling per file, in bytes read from disk. */
    const val MAX_FILE_BYTES = 4L * 1024 * 1024

    /** Hard ceiling across all attachments bundled into one prompt. */
    const val MAX_TOTAL_BYTES = 7L * 1024 * 1024

    /** `command.attachments.length` must be within 1..16. */
    const val MAX_ATTACHMENTS = 16

    fun read(
        displayName: String,
        mediaType: String,
        declaredSize: Long,
        stream: InputStream,
    ): AttachmentResult {
        // The handle is a binder file descriptor into the Documents provider,
        // and this function is the one that asked for the bytes — so it is the
        // one that closes it. A caller that only wraps the call in runCatching
        // would leak one descriptor per picked file, and a process that runs
        // out of them fails on files that are perfectly readable.
        return try {
            readOpen(displayName, mediaType, declaredSize, stream)
        } finally {
            runCatching { stream.close() }
        }
    }

    private fun readOpen(
        displayName: String,
        mediaType: String,
        declaredSize: Long,
        stream: InputStream,
    ): AttachmentResult {
        val name = displayName.trim().ifEmpty { "attachment" }
        if (name.length > 256) return AttachmentResult.Rejected("附件 $name 文件名过长")
        if (declaredSize > MAX_FILE_BYTES) {
            return AttachmentResult.Rejected("附件 $name 超过 4MB 大小限制")
        }
        val bytes =
            try {
                readCapped(stream, MAX_FILE_BYTES)
            } catch (_: IOException) {
                return AttachmentResult.Rejected("读取附件 $name 失败")
            }
            ?: return AttachmentResult.Rejected("附件 $name 超过 4MB 大小限制")
        if (bytes.isEmpty()) {
            return AttachmentResult.Rejected("附件 $name 为空文件，无法附加")
        }
        val type = mediaType.trim().ifEmpty { "application/octet-stream" }
        if (type.length > 128) return AttachmentResult.Rejected("附件 $name 类型标识过长")
        return if (isText(name, type, bytes)) {
            val text = String(bytes, StandardCharsets.UTF_8)
            if (text.length > RemoteAttachment.MAX_DATA_CHARS) {
                return AttachmentResult.Rejected("附件 $name 内容过大")
            }
            AttachmentResult.Ready(
                RemoteAttachment(
                    name = name,
                    mediaType = if (type == "application/octet-stream") "text/plain" else type,
                    data = text,
                    encoding = "utf8",
                ),
            )
        } else {
            val encoded = Base64.getEncoder().encodeToString(bytes)
            if (encoded.length > RemoteAttachment.MAX_DATA_CHARS) {
                return AttachmentResult.Rejected("附件 $name 编码后过大")
            }
            AttachmentResult.Ready(
                RemoteAttachment(
                    name = name,
                    mediaType = type,
                    data = encoded,
                    encoding = "base64",
                ),
            )
        }
    }

    /**
     * Reads at most [cap] bytes, or returns null when the file is longer.
     *
     * A file that turns out to be bigger than the cap is rejected rather than
     * truncated: sending half of a source file is worse than not sending it.
     *
     * The cap is checked *before* the bytes are buffered, not after. Writing
     * first and comparing later means a 2GB video is held in memory in full
     * before the rejection is reached, and on a phone that is the process being
     * killed rather than an error message.
     */
    private fun readCapped(stream: InputStream, cap: Long): ByteArray? {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(64 * 1024)
        var total = 0L
        while (true) {
            val read = stream.read(buffer)
            if (read < 0) break
            total += read
            if (total > cap) return null
            out.write(buffer, 0, read)
        }
        return out.toByteArray()
    }

    /**
     * True when this file should travel as UTF-8.
     *
     * The default is the other way round: a file that cannot be sent as UTF-8
     * without corrupting it goes out as base64. Base64 makes a text file 4/3
     * larger, which costs bytes; sending UTF-16 or Latin-1 as UTF-8 produces
     * mojibake, which costs the content. The two mistakes are not symmetric, so
     * the burden of proof sits on "sends cleanly".
     *
     * With bytes to look at, the bytes settle it: [decodesAsUtf8] is precisely
     * the condition under which UTF-8 transmission is lossless, so it needs no
     * help from the name or the type. An obvious binary MIME is refused first,
     * because the resolver read it from the bytes and so it outranks a filename
     * that claims otherwise.
     *
     * When no bytes were supplied — a caller classifying by metadata alone —
     * [BINARY_MIME] still refuses, and the MIME type and the extension are the
     * only evidence available. That is a weaker answer by construction: the
     * whole point of the sniff is that `text/plain` says nothing about which
     * encoding the bytes are in.
     */
    fun isText(displayName: String, mediaType: String, bytes: ByteArray = EMPTY_BYTES): Boolean {
        val type = mediaType.trim().lowercase()
        // No content to judge: an obvious binary type is still refused, and the
        // name and the type are then the only evidence there is.
        if (bytes.isEmpty()) {
            if (type in BINARY_MIME) return false
            val extension = displayName.substringAfterLast('.', "").lowercase()
            val namedText = type in TEXT_MIME_EXACT || TEXT_MIME_PREFIXES.any { type.startsWith(it) }
            return namedText || extension in TEXT_EXTENSIONS
        }
        // The bytes carry the verdict, and they are enough on their own.
        // Decoding as UTF-8 with no NUL and no replacement character is exactly
        // the condition under which sending the file as UTF-8 cannot corrupt
        // it, so a file that meets it is text whatever it is called. A binary
        // MIME short-circuits first: it is read from the bytes by the resolver
        // and is therefore stronger evidence than the name.
        if (type in BINARY_MIME) return false
        return decodesAsUtf8(bytes)
    }

    /**
     * True when [bytes] is valid UTF-8.
     *
     * UTF-16 is caught by its NUL bytes — `String(bytes, UTF_8)` turns them into
     * U+0000 and the agent sees a file with a gap between every character.
     * Latin-1 is caught by the replacement character, because every byte above
     * 0x7F is a lead byte in UTF-8 and an isolated one is malformed.
     *
     * Only the first 8KB is inspected: the check exists to classify the file, not
     * to validate it, and a multi-megabyte scan would cost more than the read it
     * follows. A file that is text up to 8KB and binary after it is a file whose
     * header lies, and the cap keeps the answer cheap.
     *
     * The window ends on the last *complete* character before the cap. Cutting a
     * multi-byte character in half leaves a truncated sequence that decodes to
     * U+FFFD, and a Chinese text file would then be judged binary on the strength
     * of an off-by-two in the cap rather than on its content — so the window is
     * retried one byte shorter until it decodes, which is at most three steps.
     */
    private fun decodesAsUtf8(bytes: ByteArray): Boolean {
        var limit = minOf(bytes.size, UTF8_SNIFF_BYTES)
        while (limit > 0) {
            val window = ByteBuffer.wrap(bytes, 0, limit)
            try {
                StandardCharsets.UTF_8.newDecoder().decode(window)
            } catch (_: CharacterCodingException) {
                // Either a truncated character at the cut, or genuinely binary.
                // Dropping the trailing byte and retrying tells the two apart:
                // a real encoding error needs more than three bytes to fix.
                limit--
                if (minOf(bytes.size, UTF8_SNIFF_BYTES) - limit > 3) return false
                continue
            }
            val text = String(bytes, 0, limit, StandardCharsets.UTF_8)
            // A NUL is legal UTF-8 and decodes fine, but no text file a person
            // picks holds one: it is the signature of UTF-16 (every other byte)
            // and of a binary with a text-looking header. Checking for it
            // explicitly is what separates "decodes" from "is text".
            if (text.indexOf(NUL) >= 0) return false
            // The replacement character means a byte the decoder had to throw
            // away. Latin-1 fills the window with them.
            return !text.contains('�')
        }
        return true
    }

    /** How many bytes [decodesAsUtf8] inspects. Visible so tests can build a file that overruns it. */
    internal const val UTF8_SNIFF_BYTES = 8 * 1024

    /** The one control character that settles "is this really text". */
    private const val NUL = 0.toChar()

    private val EMPTY_BYTES = ByteArray(0)
}

/** Human-readable size for the picker's list: `1.2 MB`. */
fun formatAttachmentSize(bytes: Long): String =
    when {
        bytes >= 1024 * 1024 -> String.format(java.util.Locale.US, "%.1f MB", bytes / (1024.0 * 1024.0))
        bytes >= 1024 -> "${bytes / 1024} KB"
        else -> "$bytes B"
    }

/**
 * Turns a batch of read results into one sentence, keeping the first failure.
 *
 * A pick can add three files and refuse two; reporting only the successes would
 * hide a file the person chose and expected to arrive, so the first refusal is
 * always surfaced and the rest are counted.
 */
fun summarizeAttachmentResults(results: List<AttachmentResult>): String? {
    val rejected = results.filterIsInstance<AttachmentResult.Rejected>()
    if (rejected.isEmpty()) return null
    val first = rejected.first().reason
    return if (rejected.size == 1) first else "$first（另有 ${rejected.size - 1} 个文件未加入）"
}
