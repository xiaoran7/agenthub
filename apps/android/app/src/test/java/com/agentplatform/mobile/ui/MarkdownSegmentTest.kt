package com.agentplatform.mobile.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The markdown splitter runs on every streaming delta, so it must produce the
 * same segments whether it parses the whole message or extends the previous
 * result. These tests pin that equivalence.
 */
class MarkdownSegmentTest {
    @Test
    fun `plain text is a single segment without touching the fence pattern`() {
        val segments = parseMarkdownSegments("第一行\n第二行")

        assertEquals(1, segments.size)
        assertFalse(segments[0].isCodeBlock)
        assertEquals("第一行\n第二行", segments[0].text)
    }

    @Test
    fun `a closed fence splits text and code`() {
        val segments = parseMarkdownSegments("看这里：\n```kotlin\nval x = 1\n```\n结束")

        assertEquals(3, segments.size)
        assertFalse(segments[0].isCodeBlock)
        assertTrue(segments[1].isCodeBlock)
        assertEquals("kotlin", segments[1].language)
        assertEquals("val x = 1\n", segments[1].text)
        assertEquals("\n结束", segments[2].text)
    }

    @Test
    fun `appending a plain delta extends the previous segment`() {
        val cache = MarkdownSegmentCache()
        val first = cache.update("正在检查")
        val second = cache.update("正在检查文件")

        assertEquals(1, second.size)
        assertEquals("正在检查文件", second[0].text)
        assertEquals("正在检查", first[0].text)
    }

    @Test
    fun `a fence split across deltas is still recognised`() {
        val cache = MarkdownSegmentCache()
        cache.update("结果：\n``")
        cache.update("结果：\n```python\nprint(1)\n```")

        val segments = cache.update("结果：\n```python\nprint(1)\n```\n完成")
        assertEquals(3, segments.size)
        assertTrue(segments[1].isCodeBlock)
        assertEquals("python", segments[1].language)
        assertEquals("\n完成", segments[2].text)
    }

    @Test
    fun `text after an open code block starts a new plain segment`() {
        val cache = MarkdownSegmentCache()
        cache.update("```sh\necho 1\n```")
        val full = "```sh\necho 1\n```\n解释"
        val segments = cache.update(full)

        assertEquals(2, segments.size)
        assertEquals("解释", segments[1].text.trim())
        // The whole point of the incremental path: it must agree with a full
        // parse, otherwise it is a rendering bug waiting to happen.
        assertEquals(parseMarkdownSegments(full), segments)
    }

    @Test
    fun `a rewritten message falls back to a full parse`() {
        val cache = MarkdownSegmentCache()
        cache.update("草稿答案")
        val segments = cache.update("最终答案 `inline`")

        assertEquals(1, segments.size)
        assertEquals("最终答案 `inline`", segments[0].text)
    }
}
