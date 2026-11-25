package com.agentplatform.mobile.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.agentplatform.mobile.data.EffortLevel
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.LaunchAgentOption
import kotlinx.coroutines.launch

// =========================================================================
// Launch controls: visual Agent selector, dock chips & bottom option sheet
// =========================================================================

internal data class LaunchSheetChoice(
    val id: String,
    val title: String,
    val subtitle: String? = null,
    val badge: String? = null,
    val enabled: Boolean = true,
)

internal fun effortHint(level: EffortLevel): String =
    when (level) {
        EffortLevel.DEFAULT -> "跟随引擎默认配置"
        EffortLevel.LOW -> "快速响应，适合轻量问答与改动"
        EffortLevel.MEDIUM -> "均衡速度与推理深度"
        EffortLevel.HIGH -> "深度推理，适合复杂排查与架构设计"
        EffortLevel.MAX -> "最大推理预算，耗时更长"
    }

@Composable
internal fun AgentEngineCardGrid(
    options: List<LaunchAgentOption>,
    selectedWire: String,
    onSelect: (LaunchAgentOption) -> Unit,
    modifier: Modifier = Modifier,
) {
    val rows = options.chunked(2)
    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
    ) {
        rows.forEach { pair ->
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                pair.forEach { option ->
                    val selected = option.wire == selectedWire
                    val cardBg by animateColorAsState(
                        targetValue =
                            if (selected) OrcaColors.accentBlue.copy(alpha = 0.11f)
                            else OrcaColors.bgPanel,
                        animationSpec = tween(MotionTokens.Quick),
                        label = "engine-card-bg",
                    )
                    val cardBorder by animateColorAsState(
                        targetValue =
                            if (selected) OrcaColors.accentBlue
                            else OrcaColors.borderSubtle,
                        animationSpec = tween(MotionTokens.Quick),
                        label = "engine-card-border",
                    )
                    Surface(
                        onClick = { onSelect(option) },
                        modifier = Modifier.weight(1f).defaultMinSize(minHeight = 66.dp),
                        shape = RoundedCornerShape(OrcaRadius.card),
                        color = cardBg,
                        border = BorderStroke(if (selected) 1.5.dp else 1.dp, cardBorder),
                    ) {
                        Row(
                            modifier = Modifier.padding(horizontal = 11.dp, vertical = 11.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(9.dp),
                        ) {
                            AgentBrandBadge(
                                engine = option.engine,
                                reportedProvider = option.wire,
                                tileSize = 34.dp,
                                iconSize = 21.dp,
                                selected = selected,
                            )
                            Column(
                                modifier = Modifier.weight(1f),
                                verticalArrangement = Arrangement.spacedBy(2.dp),
                            ) {
                                Text(
                                    text = option.label,
                                    style =
                                        MaterialTheme.typography.titleSmall.copy(
                                            fontSize = 13.5.sp,
                                            lineHeight = 17.sp,
                                            fontWeight =
                                                if (selected) FontWeight.SemiBold
                                                else FontWeight.Medium,
                                        ),
                                    color = MaterialTheme.colorScheme.onSurface,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    text =
                                        if (option.engine == EngineKind.PLUGIN) "ACP 插件"
                                        else "原生引擎",
                                    style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.5.sp),
                                    color =
                                        if (selected) OrcaColors.accentBlue
                                        else MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 1,
                                )
                            }
                            if (selected) {
                                Box(
                                    modifier =
                                        Modifier
                                            .size(18.dp)
                                            .clip(CircleShape)
                                            .background(OrcaColors.surfaceBright),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    OrcaCheckIcon(tint = OrcaColors.onBright, size = 11.dp)
                                }
                            }
                        }
                    }
                }
                if (pair.size == 1) {
                    Spacer(Modifier.weight(1f))
                }
            }
        }
    }
}

private val QUICK_START_PROMPTS =
    listOf(
        "检查当前目录 git status 并汇报最新进展",
        "运行项目测试并排查失败项",
        "梳理核心目录结构与主要入口",
    )

@Composable
internal fun LaunchQuickPromptChips(
    onPick: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    LazyRow(
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
    ) {
        items(QUICK_START_PROMPTS, key = { it }) { snippet ->
            Surface(
                onClick = { onPick(snippet) },
                modifier = Modifier.minimumInteractiveComponentSize(),
                shape = RoundedCornerShape(OrcaRadius.pill),
                color = OrcaColors.bgPanel,
                border = BorderStroke(1.dp, OrcaColors.borderSubtle),
            ) {
                Text(
                    text = snippet,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier =
                        Modifier
                            .padding(horizontal = 13.dp, vertical = 8.dp)
                            .widthIn(max = 240.dp),
                )
            }
        }
    }
}

@Composable
internal fun LaunchDockChip(
    label: String,
    value: String,
    highlighted: Boolean = false,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier.minimumInteractiveComponentSize(),
        shape = RoundedCornerShape(OrcaRadius.pill),
        color =
            if (highlighted) OrcaColors.accentBlue.copy(alpha = 0.12f)
            else OrcaColors.bgRaised,
        border =
            BorderStroke(
                1.dp,
                if (highlighted) OrcaColors.accentBlue.copy(alpha = 0.5f)
                else OrcaColors.borderSubtle,
            ),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 7.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 11.sp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = value,
                style =
                    MaterialTheme.typography.labelMedium.copy(
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Medium,
                    ),
                color =
                    if (highlighted) OrcaColors.accentBlue
                    else MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.widthIn(max = 96.dp),
            )
            OrcaChevronDown(MaterialTheme.colorScheme.onSurfaceVariant, 9.dp)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun LaunchOptionSheet(
    title: String,
    choices: List<LaunchSheetChoice>,
    selectedId: String,
    onSelect: (String) -> Unit,
    onDismiss: () -> Unit,
    headerSubtitle: String? = null,
    headerActionLabel: String? = null,
    onHeaderAction: (() -> Unit)? = null,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    var query by remember(title) { mutableStateOf("") }
    val showSearch = choices.size > 8
    val filtered =
        remember(choices, query) {
            val q = query.trim()
            if (q.isEmpty()) choices
            else choices.filter {
                it.title.contains(q, ignoreCase = true) ||
                    (it.subtitle?.contains(q, ignoreCase = true) == true) ||
                    (it.badge?.contains(q, ignoreCase = true) == true)
            }
        }

    fun dismissAnimated(after: (() -> Unit)? = null) {
        scope.launch {
            runCatching { sheetState.hide() }
            onDismiss()
            after?.invoke()
        }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = OrcaColors.bgPanel,
        contentColor = MaterialTheme.colorScheme.onSurface,
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp),
    ) {
        Column(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .navigationBarsPadding()
                    .padding(horizontal = OrcaSpace.lg)
                    .padding(bottom = OrcaSpace.lg),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(bottom = 2.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    Text(
                        text = title,
                        style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.SemiBold),
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                    if (!headerSubtitle.isNullOrBlank()) {
                        Text(
                            text = headerSubtitle,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    if (!headerActionLabel.isNullOrBlank() && onHeaderAction != null) {
                        OrcaTextAction(
                            label = headerActionLabel,
                            onClick = { dismissAnimated(after = onHeaderAction) },
                        )
                    }
                    if (showSearch) {
                        Text(
                            text = "${filtered.size}/${choices.size}",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
            if (showSearch) {
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(OrcaRadius.pill),
                    color = OrcaColors.bgRaised,
                    border = BorderStroke(1.dp, OrcaColors.borderSubtle),
                ) {
                    Row(
                        modifier =
                            Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 14.dp, vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Box(modifier = Modifier.weight(1f)) {
                            if (query.isEmpty()) {
                                Text(
                                    text = "搜索目录名、路径或电脑…",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
                                )
                            }
                            BasicTextField(
                                value = query,
                                onValueChange = { query = it },
                                singleLine = true,
                                modifier = Modifier.fillMaxWidth(),
                                textStyle =
                                    MaterialTheme.typography.bodyMedium.copy(
                                        color = MaterialTheme.colorScheme.onSurface,
                                    ),
                                cursorBrush = SolidColor(OrcaColors.accentBlue),
                            )
                        }
                        if (query.isNotEmpty()) {
                            Text(
                                text = "清除",
                                style = MaterialTheme.typography.labelSmall,
                                color = OrcaColors.accentBlue,
                                modifier = Modifier.clickable { query = "" },
                            )
                        }
                    }
                }
            }
            if (filtered.isEmpty()) {
                Text(
                    text =
                        if (choices.isEmpty()) "暂无可选项，请先在电脑端刷新配置。"
                        else "没有匹配 \"$query\" 的条目",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(vertical = OrcaSpace.md),
                )
            } else {
                LazyColumn(
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                    modifier = Modifier.fillMaxWidth().heightIn(min = 60.dp, max = 400.dp),
                ) {
                    items(filtered, key = { it.id }) { choice ->
                        val active = choice.id == selectedId
                        Surface(
                            modifier =
                                Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(OrcaRadius.tile))
                                    .clickable(enabled = choice.enabled) {
                                        onSelect(choice.id)
                                        dismissAnimated()
                                    },
                            shape = RoundedCornerShape(OrcaRadius.tile),
                            color =
                                if (active) OrcaColors.accentBlue.copy(alpha = 0.12f)
                                else OrcaColors.bgRaised.copy(alpha = 0.55f),
                            border =
                                BorderStroke(
                                    1.dp,
                                    if (active) OrcaColors.accentBlue.copy(alpha = 0.55f)
                                    else OrcaColors.borderSubtle,
                                ),
                        ) {
                            Row(
                                modifier =
                                    Modifier
                                        .fillMaxWidth()
                                        .padding(horizontal = 14.dp, vertical = 12.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(10.dp),
                            ) {
                                Column(
                                    modifier = Modifier.weight(1f),
                                    verticalArrangement = Arrangement.spacedBy(2.dp),
                                ) {
                                    Text(
                                        text = choice.title,
                                        style =
                                            MaterialTheme.typography.bodyMedium.copy(
                                                fontWeight =
                                                    if (active) FontWeight.SemiBold
                                                    else FontWeight.Medium,
                                            ),
                                        color =
                                            if (choice.enabled) MaterialTheme.colorScheme.onSurface
                                            else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    if (!choice.subtitle.isNullOrBlank()) {
                                        Text(
                                            text = choice.subtitle,
                                            style = MaterialTheme.typography.bodySmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            maxLines = 2,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                }
                                if (!choice.badge.isNullOrBlank()) {
                                    Surface(
                                        shape = RoundedCornerShape(OrcaRadius.pill),
                                        color =
                                            if (active) OrcaColors.accentBlue.copy(alpha = 0.14f)
                                            else OrcaColors.bgPanel,
                                    ) {
                                        Text(
                                            text = choice.badge,
                                            style = MaterialTheme.typography.labelSmall,
                                            color =
                                                if (active) OrcaColors.accentBlue
                                                else MaterialTheme.colorScheme.onSurfaceVariant,
                                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp),
                                        )
                                    }
                                }
                                if (active) {
                                    OrcaCheckIcon(tint = OrcaColors.accentBlue, size = 16.dp)
                                }
                            }
                        }
                    }
                }
            }
            Spacer(Modifier.height(OrcaSpace.xs))
        }
    }
}
