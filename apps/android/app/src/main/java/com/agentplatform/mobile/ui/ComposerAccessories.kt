package com.agentplatform.mobile.ui

import android.graphics.BitmapFactory
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.data.CatalogModel
import com.agentplatform.mobile.data.RemoteAttachment
import com.agentplatform.mobile.data.formatAttachmentSize
import java.util.Base64

// =========================================================================
// Composer accessories: model/option choosers and attachment preview chips
// =========================================================================

@Composable
fun <T> ChoiceRow(
    options: List<Pair<T, String>>,
    selected: T,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    LazyRow(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        items(options, key = { it.second }) { (value, label) ->
            OrcaFilterPill(
                label = label,
                active = value == selected,
                onClick = { onSelect(value) },
                enabled = enabled,
            )
        }
    }
}

@Composable
fun ModelChoiceRow(
    options: List<CatalogModel>,
    selected: String,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    if (options.isEmpty()) {
        Text(
            text = "这台电脑没有上报可选模型，将使用默认模型。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = modifier,
        )
        return
    }
    LazyRow(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        item {
            OrcaFilterPill(
                label = "默认",
                active = selected.isEmpty(),
                onClick = { onSelect("") },
                enabled = enabled,
            )
        }
        items(options, key = { it.id }) { model ->
            OrcaFilterPill(
                label = model.label,
                active = selected == model.id,
                onClick = { onSelect(model.id) },
                enabled = enabled,
            )
        }
    }
}

private fun decodeImageThumbnail(attachment: RemoteAttachment): ImageBitmap? {
    if (attachment.encoding != "base64" || !attachment.mediaType.startsWith("image/")) return null
    return runCatching {
        val bytes = Base64.getDecoder().decode(attachment.data)
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        val maxDim = maxOf(bounds.outWidth, bounds.outHeight)
        val sample = if (maxDim > 128) (maxDim / 64).coerceAtLeast(1) else 1
        val opts = BitmapFactory.Options().apply { inSampleSize = sample }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)?.asImageBitmap()
    }.getOrNull()
}

@Composable
fun AttachmentChips(
    attachments: List<RemoteAttachment>,
    onRemove: (RemoteAttachment) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (attachments.isEmpty()) return
    LazyRow(
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = OrcaSpace.lg),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
    ) {
        itemsIndexed(attachments, key = { index, _ -> index }) { _, attachment ->
            val thumbnailState =
        androidx.compose.runtime.produceState<androidx.compose.ui.graphics.ImageBitmap?>(
            initialValue = null,
            attachment.name,
            attachment.data.length,
        ) {
            value =
                kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Default) {
                    decodeImageThumbnail(attachment)
                }
        }
    val thumbnail = thumbnailState.value
            Surface(
                shape = RoundedCornerShape(OrcaRadius.button),
                color = MaterialTheme.colorScheme.surfaceVariant,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
            ) {
                Row(
                    modifier =
                        Modifier
                            .heightIn(min = OrcaSpace.touch)
                            .clickable { onRemove(attachment) }
                            .padding(horizontal = OrcaSpace.sm, vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    if (thumbnail != null) {
                        Image(
                            bitmap = thumbnail,
                            contentDescription = attachment.name,
                            contentScale = ContentScale.Crop,
                            modifier =
                                Modifier
                                    .size(28.dp)
                                    .clip(RoundedCornerShape(6.dp)),
                        )
                    }
                    Text(
                        text = attachment.name,
                        style = OrcaType.mono,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.widthIn(max = 140.dp),
                    )
                    Text(
                        text =
                            formatAttachmentSize(attachment.estimatedByteLength) +
                                if (attachment.encoding == "base64") {
                                    if (attachment.mediaType.startsWith("image/")) " · 图片" else " · 二进制"
                                } else {
                                    " · 文本"
                                },
                        style = OrcaType.mono,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                    )
                    Text(
                        text = "移除",
                        style = OrcaType.mono,
                        color = OrcaColors.accentRed,
                        maxLines = 1,
                    )
                }
            }
        }
    }
}

@Composable
fun OrcaAttachIcon(
    tint: Color,
    modifier: Modifier = Modifier,
    size: Dp = 16.dp,
) {
    Box(modifier = modifier.size(size), contentAlignment = Alignment.Center) {
        Canvas(Modifier.size(size)) {
            val w = this.size.width
            val h = this.size.height
            val stroke = 1.6.dp.toPx()
            val path =
                Path().apply {
                    moveTo(w * 0.24f, h * 0.78f)
                    lineTo(w * 0.24f, h * 0.34f)
                    cubicTo(w * 0.24f, h * 0.16f, w * 0.34f, h * 0.10f, w * 0.50f, h * 0.10f)
                    cubicTo(w * 0.66f, h * 0.10f, w * 0.76f, h * 0.16f, w * 0.76f, h * 0.34f)
                    lineTo(w * 0.76f, h * 0.62f)
                }
            drawPath(
                path,
                color = tint,
                style =
                    Stroke(
                        width = stroke,
                        cap = StrokeCap.Round,
                        join = StrokeJoin.Round,
                    ),
            )
        }
    }
}

@Composable
fun AttachmentNotice(
    message: String,
    modifier: Modifier = Modifier,
) {
    Text(
        text = message,
        style = MaterialTheme.typography.bodySmall,
        color = OrcaColors.accentAmber,
        modifier = modifier,
    )
}
