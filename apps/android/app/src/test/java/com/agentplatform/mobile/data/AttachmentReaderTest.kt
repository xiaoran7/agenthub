package com.agentplatform.mobile.data

import com.agentplatform.mobile.ui.estimatedDecodedBytes
import java.io.ByteArrayInputStream
import java.io.IOException
import java.io.InputStream
import java.nio.charset.StandardCharsets
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Attachment reader tests.
 *
 * The reader is pure logic over a stream, so every case runs without a device:
 * what matters is that a file which cannot travel is refused *before* the
 * prompt is built, and that the refusal names the file.
 */
class AttachmentReaderTest {
    private fun read(
        name: String,
        bytes: ByteArray,
        mediaType: String = "",
        declaredSize: Long = bytes.size.toLong(),
    ) = AttachmentReader.read(
        displayName = name,
        mediaType = mediaType,
        declaredSize = declaredSize,
        stream = ByteArrayInputStream(bytes),
    )

    // ------------------------------------------------------------------ text

    @Test
    fun `a text file travels as utf8`() {
        val result = read("notes.md", "hello 世界".toByteArray())

        val attachment = (result as AttachmentResult.Ready).attachment
        assertEquals("notes.md", attachment.name)
        assertEquals("utf8", attachment.encoding)
        assertEquals("hello 世界", attachment.data)
        assertTrue(attachment.fitsLimit())
    }

    @Test
    fun `a text mime is honoured over the extension`() {
        val result = read("mystery", "plain".toByteArray(), mediaType = "text/plain")
        assertEquals("utf8", (result as AttachmentResult.Ready).attachment.encoding)
    }

    @Test
    fun `an octet-stream with no extension is treated as binary`() {
        val result = read("blob", byteArrayOf(0, 1, 2, 3))

        val attachment = (result as AttachmentResult.Ready).attachment
        assertEquals("base64", attachment.encoding)
        assertEquals("application/octet-stream", attachment.mediaType)
    }

    @Test
    fun `a text file with no type at all is labelled text`() {
        val result = read("readme.txt", "hi".toByteArray())
        assertEquals("text/plain", (result as AttachmentResult.Ready).attachment.mediaType)
    }

    @Test
    fun `a binary file is base64 encoded`() {
        val bytes = byteArrayOf(0xDE.toByte(), 0xAD.toByte(), 0xBE.toByte(), 0xEF.toByte())
        val result = read("image.png", bytes, mediaType = "image/png")

        val attachment = (result as AttachmentResult.Ready).attachment
        assertEquals("base64", attachment.encoding)
        assertEquals("3q2+7w==", attachment.data)
    }

    @Test
    fun `an image named as text still travels as base64`() {
        // The name is what the file is called, not what it holds. This is the
        // case the classification rule exists for, checked end to end through
        // the reader rather than only through `isText`.
        val bytes = byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A)
        val result = read("notes.txt", bytes, mediaType = "image/png")

        val attachment = (result as AttachmentResult.Ready).attachment
        assertEquals("base64", attachment.encoding)
        assertEquals("image/png", attachment.mediaType)
    }

    // ----------------------------------------------------------------- limits

    @Test
    fun `a declared size over the ceiling is refused before any read`() {
        val result = read("huge.bin", ByteArray(16), declaredSize = AttachmentReader.MAX_FILE_BYTES + 1)

        val reason = (result as AttachmentResult.Rejected).reason
        assertTrue(reason.contains("huge.bin"))
        assertTrue(reason.contains("4MB"))
    }

    @Test
    fun `a file that lies about its size is refused on the real length`() {
        // A cloud-backed picker reports 0 until the file is downloaded, so the
        // declared size cannot be the only check.
        val big = ByteArray((AttachmentReader.MAX_FILE_BYTES + 2).toInt()) { 0x61 }
        val result = read("huge.bin", big, declaredSize = 0)

        assertTrue(result is AttachmentResult.Rejected)
    }

    @Test
    fun `a blank name falls back to a usable label`() {
        val result = read("", "hi".toByteArray())
        assertEquals("attachment", (result as AttachmentResult.Ready).attachment.name)
    }

    @Test
    fun `a name over the wire limit is refused`() {
        val result = read("x".repeat(300), "hi".toByteArray())
        assertTrue((result as AttachmentResult.Rejected).reason.contains("文件名过长"))
    }

    @Test
    fun `an unreadable stream is reported as a failure not a crash`() {
        val result =
            AttachmentReader.read(
                displayName = "broken.txt",
                mediaType = "text/plain",
                declaredSize = 4,
                stream =
                    object : InputStream() {
                        override fun read(): Int = throw IOException("provider gone")
                    },
            )

        assertTrue((result as AttachmentResult.Rejected).reason.contains("broken.txt"))
    }

    // ------------------------------------------------------------ classification

    @Test
    fun `text detection follows the extension for a file the provider cannot type`() {
        assertTrue(AttachmentReader.isText("a.kt", ""))
        assertTrue(AttachmentReader.isText("a.json", ""))
        assertTrue(AttachmentReader.isText("A.KT", ""))
        assertFalse(AttachmentReader.isText("a.png", ""))
        assertFalse(AttachmentReader.isText("a", ""))
    }

    @Test
    fun `text detection follows the mime when the name is unhelpful`() {
        assertTrue(AttachmentReader.isText("a", "text/x-python"))
        assertTrue(AttachmentReader.isText("a", "application/json"))
        assertFalse(AttachmentReader.isText("a", "application/octet-stream"))
    }

    @Test
    fun `a binary mime outranks a text extension`() {
        // The resolver reads the bytes; the name is only what the file is
        // called. An image saved as .txt sent as UTF-8 would arrive as
        // mojibake, which is worse than an attachment the agent can decode.
        //
        // No bytes supplied here: this is the metadata-only path, where the
        // binary MIME is the strongest evidence available. With bytes it is a
        // short-circuit before the sniff, not the verdict — see the cases below.
        assertFalse(AttachmentReader.isText("photo.txt", "image/png"))
        assertFalse(AttachmentReader.isText("data.json", "application/pdf"))
        assertFalse(AttachmentReader.isText("clip.kt", "video/mp4"))
    }

    @Test
    fun `an unknown application type is left to the extension`() {
        // Refusing to guess here would base64 a source file the resolver
        // mislabelled, for no benefit.
        assertTrue(AttachmentReader.isText("main.kt", "application/x-something"))
    }

    @Test
    fun `an octet-stream name falls back to the extension rather than binary`() {
        // A blank MIME is substituted with octet-stream, and the extension is
        // then the only honest signal available.
        assertTrue(AttachmentReader.isText("readme.md", "application/octet-stream"))
    }

    @Test
    fun `a utf16 text file is caught by its bytes not by its mime`() {
        // `text/plain` does not say which encoding the bytes are in. Decoding
        // UTF-16 as UTF-8 yields a NUL between every character, so the agent
        // receives a file with a hole punched through it — the exact failure the
        // binary list exists to prevent, arriving through the text door.
        val utf16 = "hello 世界".toByteArray(StandardCharsets.UTF_16)
        assertFalse(AttachmentReader.isText("notes.txt", "text/plain", utf16))
        assertFalse(AttachmentReader.isText("notes.kt", "", utf16))
    }

    @Test
    fun `a latin1 text file is caught by its bytes`() {
        // Latin-1 bytes above 0x7F are isolated in UTF-8 and decode to U+FFFD,
        // which is mojibake rather than text.
        val latin1 = "café résumé".toByteArray(StandardCharsets.ISO_8859_1)
        assertFalse(AttachmentReader.isText("notes.txt", "text/plain", latin1))
    }

    @Test
    fun `a plain ascii file travels as text`() {
        val ascii = "the quick brown fox\njumps over\n".toByteArray()
        assertTrue(AttachmentReader.isText("notes.txt", "text/plain", ascii))
        assertTrue(AttachmentReader.isText("main.kt", "", ascii))
        // No MIME and no extension either: the bytes are the whole answer.
        assertTrue(AttachmentReader.isText("notes", "application/octet-stream", ascii))
    }

    @Test
    fun `a binary wearing a text name is caught by its NUL bytes`() {
        // A text-looking header followed by NULs: decodes as UTF-8, so only the
        // explicit NUL check separates it from text.
        val fakeText = byteArrayOf(0x50, 0x4B, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00) + "padding".toByteArray()
        assertFalse(AttachmentReader.isText("notes.txt", "text/plain", fakeText))
    }

    @Test
    fun `clean ascii bytes settle it without any help from the name`() {
        // The name and type say nothing, but "ABCD" is ASCII: there is no way to
        // corrupt it in UTF-8 transmission, so there is nothing left to be
        // unsure about. Requiring a text-shaped filename on top of the bytes
        // would base64 this for no benefit.
        val opaque = byteArrayOf(0x41, 0x42, 0x43, 0x44)
        assertTrue(AttachmentReader.isText("blob", "application/octet-stream", opaque))
        assertTrue(AttachmentReader.isText("blob.dat", "application/x-unknown", opaque))
    }

    @Test
    fun `a utf8 file with a bom travels as text`() {
        // A BOM is three valid UTF-8 bytes, so it must not trip the sniff.
        val bom = byteArrayOf(0xEF.toByte(), 0xBB.toByte(), 0xBF.toByte()) + "hi".toByteArray()
        assertTrue(AttachmentReader.isText("notes.txt", "text/plain", bom))
    }

    @Test
    fun `a utf16 file is base64 encoded end to end`() {
        val utf16 = "hello 世界".toByteArray(StandardCharsets.UTF_16)
        val result = read("notes.txt", utf16, mediaType = "text/plain")

        val attachment = (result as AttachmentResult.Ready).attachment
        assertEquals("base64", attachment.encoding)
        assertEquals("text/plain", attachment.mediaType)
    }

    @Test
    fun `binary bytes inside the sniff window are caught past the header`() {
        // The sniff only reads the first 8KB, so a file that is text for its
        // whole first 8KB and binary afterwards travels as text. That is the
        // documented trade: the check classifies, it does not validate, and a
        // multi-megabyte scan would cost more than the read it follows. This
        // proves the bytes reached the check at all rather than the MIME
        // short-circuiting it.
        val head = "x".repeat(AttachmentReader.UTF8_SNIFF_BYTES - 4).toByteArray()
        val tail = byteArrayOf(0xFF.toByte(), 0xFE.toByte(), 0xFD.toByte(), 0xFC.toByte())
        assertFalse(AttachmentReader.isText("notes.txt", "text/plain", head + tail))
    }

    @Test
    fun `a file that is text throughout the window travels as text`() {
        // The cap must not turn a large but perfectly ordinary file into
        // base64: 20KB of valid UTF-8 is text even though only the first 8KB
        // is inspected.
        val big = "世界".repeat(AttachmentReader.UTF8_SNIFF_BYTES).toByteArray()
        assertTrue(AttachmentReader.isText("notes.txt", "text/plain", big))
    }

    @Test
    fun `a multi byte character straddling the cap does not read as binary`() {
        // The window is pulled back to a character boundary, so a 3-byte CJK
        // character that happens to sit across the 8KB cut is not judged binary
        // on the strength of an off-by-three. Without that, every large Chinese
        // text file would be base64'd.
        val filler = "a".repeat(AttachmentReader.UTF8_SNIFF_BYTES - 1)
        val bytes = (filler + "世界").toByteArray()
        // The cut lands one byte into a 3-byte character.
        assertTrue(bytes[AttachmentReader.UTF8_SNIFF_BYTES].toInt() and 0xC0 == 0x80)
        assertTrue(AttachmentReader.isText("notes.txt", "text/plain", bytes))
    }

    // ------------------------------------------------------------- summarising

    @Test
    fun `a clean pick produces no notice`() {
        assertTrue(summarizeAttachmentResults(listOf(AttachmentResult.Ready(attachment("a.txt")))) == null)
    }

    @Test
    fun `a single refusal is shown verbatim`() {
        val results =
            listOf(
                AttachmentResult.Ready(attachment("a.txt")),
                AttachmentResult.Rejected("附件 b.png 超过 4MB 大小限制"),
            )

        assertEquals("附件 b.png 超过 4MB 大小限制", summarizeAttachmentResults(results))
    }

    @Test
    fun `several refusals name the first and count the rest`() {
        val results =
            listOf(
                AttachmentResult.Rejected("附件 a.png 超过 4MB 大小限制"),
                AttachmentResult.Rejected("附件 b.png 超过 4MB 大小限制"),
                AttachmentResult.Rejected("附件 c.png 超过 4MB 大小限制"),
            )

        assertEquals(
            "附件 a.png 超过 4MB 大小限制（另有 2 个文件未加入）",
            summarizeAttachmentResults(results),
        )
    }

    @Test
    fun `sizes read as human units`() {
        assertEquals("512 B", formatAttachmentSize(512))
        assertEquals("2 KB", formatAttachmentSize(2048))
        assertEquals("1.5 MB", formatAttachmentSize(1024 * 1024 * 3 / 2))
    }

    @Test
    fun `empty files are rejected and decoded byte estimation handles utf8 and base64`() {
        val emptyResult =
            AttachmentReader.read(
                displayName = "empty.txt",
                mediaType = "text/plain",
                declaredSize = 0L,
                stream = ByteArrayInputStream(ByteArray(0)),
            )
        assertTrue(emptyResult is AttachmentResult.Rejected)
        assertEquals(7L * 1024 * 1024, AttachmentReader.MAX_TOTAL_BYTES)

        val utf8 = RemoteAttachment("a.txt", "text/plain", "hello 世界", "utf8")
        assertEquals("hello 世界".toByteArray(StandardCharsets.UTF_8).size.toLong(), utf8.estimatedDecodedBytes())

        val b64 = RemoteAttachment("b.png", "image/png", "A".repeat(400), "base64")
        assertEquals(300L, b64.estimatedDecodedBytes())
    }

    private fun attachment(name: String) =
        RemoteAttachment(
            name = name,
            mediaType = "text/plain",
            data = "x",
            encoding = "utf8",
        )
}
