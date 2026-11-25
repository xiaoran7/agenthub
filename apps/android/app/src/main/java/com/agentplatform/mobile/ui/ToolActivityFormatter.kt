package com.agentplatform.mobile.ui

import org.json.JSONArray
import org.json.JSONObject

// =========================================================================
// Semantic tool formatter: turns raw JSON/shell tool records into concise,
// human-readable cards without exposing `{`, `@'`, or duplicate lines.
// =========================================================================

internal enum class ToolCategory {
    COMMAND,
    FILE_READ,
    FILE_EDIT,
    SEARCH,
    WEB,
    FOCUS,
    SUBAGENT,
    MCP,
    OTHER,
}

internal data class ToolParamRow(
    val label: String,
    val value: String,
)

internal data class FormattedToolActivity(
    val category: ToolCategory,
    val title: String,
    val subtitle: String?,
    val rawToolName: String?,
    val description: String?,
    val diff: String?,
    val diffStats: Pair<Int, Int>?,
    val params: List<ToolParamRow>,
    val commandCode: String?,
    val rawCommandForCopy: String?,
    val outputPreview: String?,
    val rawOutputForCopy: String?,
    val outputIsTrivialOk: Boolean,
) {
    val hasDetail: Boolean
        get() =
            description != null ||
                diff != null ||
                params.isNotEmpty() ||
                commandCode != null ||
                (outputPreview != null && !outputIsTrivialOk)
}

private val BOILERPLATE_FIRST_LINES =
    setOf("{", "}", "[", "]", "@'", "@\"", "'''", "\"\"\"", "```", "set -e", "set -euo pipefail")

private val KEY_LABEL_MAP =
    mapOf(
        "focus" to "当前焦点",
        "toolsummary" to "摘要",
        "description" to "说明",
        "path" to "文件路径",
        "file_path" to "文件路径",
        "filepath" to "文件路径",
        "absolutepath" to "文件路径",
        "targetfile" to "目标文件",
        "startline" to "起始行",
        "endline" to "结束行",
        "commandline" to "执行命令",
        "command" to "执行命令",
        "cmd" to "执行命令",
        "query" to "搜索词",
        "pattern" to "匹配模式",
        "url" to "网址",
        "prompt" to "任务提示",
        "role" to "角色",
        "typename" to "类型",
        "action" to "操作",
        "cwd" to "目录",
        "searchpath" to "搜索路径",
        "searchdirectory" to "搜索目录",
        "steps" to "步骤列表",
    )

private val HIDDEN_PARAM_KEYS =
    setOf("waitmsbeforeasync", "notificationtimeoutseconds", "isdaemon", "byteoffset")

private val TOOL_HEAD_REGEX = Regex("^[a-zA-Z0-9_.-]+(?:\\s*·\\s*[a-zA-Z0-9_.-]+)?$")

internal fun formatToolActivity(item: TranscriptItem): FormattedToolActivity {
    val rawTitle = item.title?.trim().orEmpty()
    val rawCommand = item.command?.trim()?.takeIf { it.isNotEmpty() }
    val rawOutput = item.output?.trim()?.takeIf { it.isNotEmpty() }
    val rawDescription = item.description?.trim()?.takeIf { it.isNotEmpty() }

    val (toolKey, titleInlineArg) = extractToolKeyAndInlineArg(rawTitle)
    val category = classifyTool(toolKey, rawTitle, rawCommand)
    val parsedJson = parseJsonObject(rawCommand)

    val diffText = rawCommand?.takeIf { isDiffPayload(it) }
    val diffStats = diffText?.let(::computeDiffLineCounts)

    val params =
        if (parsedJson != null && diffText == null) {
            extractReadableParams(parsedJson)
        } else {
            emptyList()
        }

    val jsonCommandLine =
        parsedJson?.let { firstStringField(it, listOf("CommandLine", "command", "cmd")) }
    val commandCode =
        when {
            diffText != null -> null
            jsonCommandLine != null -> readableText(jsonCommandLine)
            parsedJson != null && params.isNotEmpty() -> null
            rawCommand != null -> readableText(rawCommand)
            else -> null
        }

    val friendlyTitle = buildFriendlyTitle(category, toolKey, rawTitle, parsedJson)
    val rawSubtitle =
        extractSubtitle(
            category = category,
            description = rawDescription,
            parsedJson = parsedJson,
            titleInlineArg = titleInlineArg,
            rawCommand = rawCommand,
            diffText = diffText,
        )
    val cleanSubtitle =
        rawSubtitle
            ?.let(::readableText)
            ?.trim()
            ?.takeIf { it.isNotEmpty() && !it.equals(friendlyTitle, ignoreCase = true) }
            ?.take(120)

    val trivialOk =
        rawOutput != null &&
            rawOutput.length <= 8 &&
            rawOutput.lowercase() in setOf("ok", "done", "true", "success")

    val normalizedToolBadge =
        toolKey.takeIf { it.isNotEmpty() && !it.equals(friendlyTitle, ignoreCase = true) }

    return FormattedToolActivity(
        category = category,
        title = friendlyTitle,
        subtitle = cleanSubtitle,
        rawToolName = normalizedToolBadge,
        description =
            rawDescription?.let(::readableText)?.takeIf { it != cleanSubtitle && it != friendlyTitle },
        diff = diffText,
        diffStats = diffStats,
        params = params,
        commandCode = commandCode?.let(::readableOutput),
        rawCommandForCopy = jsonCommandLine ?: rawCommand,
        outputPreview = rawOutput?.let { readableOutput(readableText(it)) },
        rawOutputForCopy = rawOutput,
        outputIsTrivialOk = trivialOk,
    )
}

internal fun summarizeToolGroup(items: List<TranscriptItem>): String {
    if (items.isEmpty()) return "工具调用"
    val running = items.count { it.state == ToolState.RUNNING }
    val failed = items.count { it.state == ToolState.FAILED }
    val sampleNames =
        items
            .map { formatToolActivity(it).title }
            .distinct()
            .take(2)
            .joinToString("、")
    return buildString {
        append("执行工具 ${items.size} 次")
        if (sampleNames.isNotEmpty()) append(" · $sampleNames")
        if (running > 0) append(" · ${running} 项执行中")
        if (failed > 0) append(" · ${failed} 项失败")
    }
}

private fun extractToolKeyAndInlineArg(rawTitle: String): Pair<String, String?> {
    if (rawTitle.isEmpty()) return "" to null
    val colonIdx = rawTitle.indexOf(':')
    if (colonIdx in 1..48) {
        val head = rawTitle.substring(0, colonIdx).trim()
        val tail = rawTitle.substring(colonIdx + 1).trim().takeIf { it.isNotEmpty() }
        if (TOOL_HEAD_REGEX.matches(head)) {
            return head to tail
        }
    }
    return rawTitle to null
}

private fun classifyTool(toolKey: String, rawTitle: String, rawCommand: String?): ToolCategory {
    val norm = toolKey.lowercase().substringAfterLast("·").trim()
    val titleNorm = rawTitle.lowercase()
    return when {
        norm in setOf("run_command", "bash", "shell", "exec_command", "terminal", "powershell") ||
            titleNorm.startsWith("命令执行") -> ToolCategory.COMMAND
        norm in setOf("read_file", "view_file", "read", "view_file_outline", "view_code_item") ||
            titleNorm.startsWith("读取文件") -> ToolCategory.FILE_READ
        norm in setOf(
            "edit_file",
            "write_file",
            "edit",
            "write",
            "replace_file_content",
            "multi_replace_file_content",
            "apply_patch",
            "notebookedit",
        ) || titleNorm.startsWith("文件修改") || (rawCommand != null && isDiffPayload(rawCommand)) ->
            ToolCategory.FILE_EDIT
        norm in setOf("grep_search", "find_by_name", "grep", "glob", "list_dir", "ls", "search") ->
            ToolCategory.SEARCH
        norm in setOf("search_web", "read_url_content", "websearch", "webfetch", "fetch") ->
            ToolCategory.WEB
        norm in setOf("update_focus", "update_steps", "todowrite", "todo_write") ->
            ToolCategory.FOCUS
        norm in setOf("invoke_subagent", "define_subagent", "send_message", "agent", "task") ->
            ToolCategory.SUBAGENT
        norm.startsWith("mcp_") || toolKey.contains("·") -> ToolCategory.MCP
        else -> ToolCategory.OTHER
    }
}

private fun buildFriendlyTitle(
    category: ToolCategory,
    toolKey: String,
    rawTitle: String,
    json: JSONObject?,
): String {
    val summary = json?.let { firstStringField(it, listOf("toolSummary")) }?.takeIf { it.length <= 36 }
    if (summary != null && category == ToolCategory.COMMAND) return summary

    val norm = toolKey.lowercase().substringAfterLast("·").trim()
    val mapped =
        when (norm) {
            "update_focus" -> "更新当前焦点"
            "update_steps" -> "更新执行步骤"
            "run_command", "bash", "shell", "exec_command" -> "运行命令"
            "read_file", "view_file", "read" -> "读取文件"
            "write_file", "write" -> "写入文件"
            "edit_file", "edit", "replace_file_content", "multi_replace_file_content" -> "编辑文件"
            "grep_search", "grep" -> "搜索代码"
            "find_by_name", "glob" -> "查找文件"
            "list_dir", "ls" -> "浏览目录"
            "search_web", "websearch" -> "网页搜索"
            "read_url_content", "webfetch" -> "读取网页"
            "invoke_subagent", "agent", "task" -> "调用子智能体"
            "manage_task" -> "管理后台任务"
            "manage_subagents" -> "管理子智能体"
            "ask_question" -> "发起确认提问"
            else -> null
        }
    if (mapped != null) return mapped
    if (rawTitle.startsWith("文件修改") || rawTitle.startsWith("命令执行")) return rawTitle
    if (toolKey.isNotEmpty() && toolKey != rawTitle) return toolKey
    return rawTitle.ifBlank { "工具调用" }
}

private fun extractSubtitle(
    category: ToolCategory,
    description: String?,
    parsedJson: JSONObject?,
    titleInlineArg: String?,
    rawCommand: String?,
    diffText: String?,
): String? {
    if (!description.isNullOrBlank()) return description

    if (parsedJson != null) {
        parsedJson.optJSONArray("steps")?.let { steps ->
            return summarizeStepsArray(steps)
        }
        firstStringField(
            parsedJson,
            listOf("focus", "toolSummary", "description", "summary", "Role", "Prompt"),
        )?.let { return it }

        val pathVal =
            firstStringField(parsedJson, listOf("Path", "path", "file_path", "filePath", "AbsolutePath", "TargetFile"))
        if (pathVal != null) {
            val shortName = shortPathTail(pathVal)
            val startLine = parsedJson.optInt("StartLine", -1).takeIf { it > 0 }
            val endLine = parsedJson.optInt("EndLine", -1).takeIf { it > 0 }
            return when {
                startLine != null && endLine != null -> "$shortName (L$startLine–$endLine)"
                startLine != null -> "$shortName (L$startLine+)"
                else -> shortName
            }
        }
        firstStringField(parsedJson, listOf("CommandLine", "command", "cmd"))?.let { cmd ->
            return extractMeaningfulCodeLine(cmd)
        }
        firstStringField(parsedJson, listOf("query", "Query", "pattern", "Pattern", "url", "Url"))?.let {
            return it
        }
        val keys = parsedJson.keys()
        while (keys.hasNext()) {
            val k = keys.next()
            if (k.lowercase() in HIDDEN_PARAM_KEYS) continue
            val v = parsedJson.opt(k)
            if (v is String && v.isNotBlank()) return extractMeaningfulCodeLine(v) ?: v.trim()
        }
    }

    if (diffText != null) {
        val headerFile =
            diffText.lineSequence()
                .firstOrNull { it.startsWith("+++ ") }
                ?.removePrefix("+++ ")
                ?.removePrefix("b/")
                ?.trim()
        if (!headerFile.isNullOrBlank() && headerFile != "/dev/null") {
            return shortPathTail(headerFile)
        }
    }

    if (rawCommand != null) {
        extractMeaningfulCodeLine(rawCommand)?.let { return it }
    }

    if (titleInlineArg != null && !isBoilerplateLine(titleInlineArg)) {
        return titleInlineArg
    }

    return if (category == ToolCategory.COMMAND) "执行终端脚本" else null
}

private fun summarizeStepsArray(steps: JSONArray): String? {
    val total = steps.length()
    if (total == 0) return null
    var done = 0
    var currentTitle: String? = null
    for (i in 0 until total) {
        val obj = steps.optJSONObject(i) ?: continue
        val status = obj.optString("status", "")
        if (status == "done") done++
        if (status == "in_progress" && currentTitle == null) {
            currentTitle = obj.optString("title", "").trim().takeIf { it.isNotEmpty() }
        }
    }
    return buildString {
        append("$done/$total 已完成")
        if (currentTitle != null) append(" · $currentTitle")
    }
}

internal fun extractMeaningfulCodeLine(raw: String): String? {
    val lines = raw.lineSequence().map { it.trim() }.filter { it.isNotEmpty() }.toList()
    if (lines.isEmpty()) return null
    for (line in lines) {
        if (isBoilerplateLine(line)) continue
        if (line.startsWith("#!") && lines.size > 1) continue
        if (line == "'@" || line == "\"@") continue
        return line
    }
    return lines.firstOrNull { !isBoilerplateLine(it) }
}

private fun isBoilerplateLine(line: String): Boolean {
    val t = line.trim().trimEnd('…', '.')
    if (t.isEmpty()) return true
    if (t in BOILERPLATE_FIRST_LINES) return true
    if (t.startsWith("<<") && t.length <= 12) return true
    if (t.all { it in "{}[]()@'\"` ,;:" }) return true
    return false
}

private fun parseJsonObject(raw: String?): JSONObject? {
    val trimmed = raw?.trim() ?: return null
    if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return null
    return runCatching { JSONObject(trimmed) }.getOrNull()
}

private fun firstStringField(obj: JSONObject, keys: List<String>): String? {
    for (k in keys) {
        if (obj.has(k) && !obj.isNull(k)) {
            val v = obj.optString(k, "").trim()
            if (v.isNotEmpty()) return v
        }
    }
    return null
}

private fun extractReadableParams(obj: JSONObject): List<ToolParamRow> {
    val rows = mutableListOf<ToolParamRow>()
    val iter = obj.keys()
    while (iter.hasNext()) {
        val key = iter.next()
        val norm = key.lowercase()
        if (norm in HIDDEN_PARAM_KEYS) continue
        if (norm in setOf("commandline", "command", "cmd")) continue
        if (obj.isNull(key)) continue
        val rawVal = obj.opt(key) ?: continue
        val label = KEY_LABEL_MAP[norm] ?: key
        val formatted =
            when (rawVal) {
                is String -> rawVal.trim().takeIf { it.isNotEmpty() }?.let(::readableText)
                is Boolean -> if (rawVal) "是" else "否"
                is Number -> rawVal.toString()
                is JSONArray ->
                    if (norm == "steps") summarizeStepsArray(rawVal)
                    else {
                        val items = (0 until rawVal.length()).mapNotNull { idx ->
                            rawVal.optString(idx, "").trim().takeIf { it.isNotEmpty() }
                        }
                        if (items.isEmpty()) null else items.joinToString("、")
                    }
                else -> rawVal.toString().takeIf { it.isNotBlank() && it != "{}" }
            }
        if (formatted != null) {
            rows += ToolParamRow(label = label, value = formatted.take(400))
        }
    }
    return rows
}

private fun shortPathTail(rawPath: String): String {
    val clean = readableText(rawPath).replace('\\', '/').trimEnd('/')
    val parts = clean.split('/').filter { it.isNotEmpty() }
    return when {
        parts.size >= 2 -> "${parts[parts.size - 2]}/${parts.last()}"
        parts.size == 1 -> parts.first()
        else -> clean
    }
}

private fun isDiffPayload(raw: String): Boolean {
    val t = raw.trim()
    return (raw.contains("\n+") && (raw.contains("\n-") || t.startsWith("---") || t.startsWith("@@"))) ||
        t.startsWith("---") ||
        t.startsWith("@@") ||
        (t.startsWith("{") && (t.contains("\"old_string\"") || t.contains("\"new_string\"") || t.contains("\"oldText\"")))
}

private fun computeDiffLineCounts(raw: String): Pair<Int, Int>? {
    val normalized = normalizeDiffText(raw)
    var adds = 0
    var dels = 0
    for (line in normalized.lineSequence()) {
        if (line.startsWith("+++") || line.startsWith("---") || line.startsWith("@@")) continue
        if (line.startsWith("+")) adds++
        else if (line.startsWith("-")) dels++
    }
    return if (adds > 0 || dels > 0) adds to dels else null
}
