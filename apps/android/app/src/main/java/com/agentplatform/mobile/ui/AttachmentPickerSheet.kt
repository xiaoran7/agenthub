package com.agentplatform.mobile.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import com.agentplatform.mobile.data.AttachmentReader
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.launch

// =========================================================================
// Shared Attachment Picker: photo library, camera capture, and file picker
// =========================================================================

internal class AttachmentPickerController(
    val pickPhotos: () -> Unit,
    val takePhoto: () -> Unit,
    val pickFiles: () -> Unit,
)

@Composable
internal fun rememberAttachmentPicker(
    onAttach: (List<Uri>) -> Unit,
    onError: (String) -> Unit = {},
): AttachmentPickerController {
    val context = LocalContext.current
    var pendingCameraPath by rememberSaveable { mutableStateOf<String?>(null) }

    val fileLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { picked ->
            if (picked.isNotEmpty()) onAttach(picked)
        }
    val photoLauncher =
        rememberLauncherForActivityResult(
            ActivityResultContracts.PickMultipleVisualMedia(AttachmentReader.MAX_ATTACHMENTS),
        ) { picked ->
            if (picked.isNotEmpty()) onAttach(picked)
        }
    val cameraLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { captured ->
            val file = pendingCameraPath?.let(::File)
            pendingCameraPath = null
            if (captured && file != null && file.exists() && file.length() > 0L) {
                runCatching {
                    FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
                }.onSuccess { uri ->
                    onAttach(listOf(uri))
                }.onFailure {
                    runCatching { file.delete() }
                    onError("无法读取刚拍摄的照片，请改用相册图片")
                }
            } else {
                runCatching { file?.delete() }
            }
        }

    return remember(context, onAttach, onError) {
        AttachmentPickerController(
            pickPhotos = {
                try {
                    photoLauncher.launch(
                        PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly),
                    )
                } catch (_: ActivityNotFoundException) {
                    try {
                        fileLauncher.launch(arrayOf("image/*"))
                    } catch (_: ActivityNotFoundException) {
                        onError("当前设备未找到可用的图片选择器")
                    }
                }
            },
            takePhoto = {
                var targetFile: File? = null
                try {
                    val created = createCameraCaptureTarget(context)
                    targetFile = created.second
                    pendingCameraPath = created.second.absolutePath
                    cameraLauncher.launch(created.first)
                } catch (_: ActivityNotFoundException) {
                    runCatching { targetFile?.delete() }
                    pendingCameraPath = null
                    onError("当前设备未找到可用的相机应用")
                } catch (_: Exception) {
                    runCatching { targetFile?.delete() }
                    pendingCameraPath = null
                    onError("无法启动拍照，请改用相册图片")
                }
            },
            pickFiles = {
                try {
                    fileLauncher.launch(arrayOf("*/*"))
                } catch (_: ActivityNotFoundException) {
                    onError("当前设备未找到可用的文件管理器")
                }
            },
        )
    }
}

private fun createCameraCaptureTarget(context: Context): Pair<Uri, File> {
    val dir = File(context.cacheDir, "camera_captures").apply { mkdirs() }
    // Prune stale temporary captures older than 2 hours
    val cutoff = System.currentTimeMillis() - 2L * 60 * 60 * 1000
    dir.listFiles()?.forEach { old ->
        if (old.isFile && old.lastModified() < cutoff) runCatching { old.delete() }
    }
    val stamp = SimpleDateFormat("yyyyMMdd_HHmmss_SSS", Locale.US).format(Date())
    val file = File(dir, "photo_$stamp.jpg")
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
    return uri to file
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AttachmentPickerSheet(
    controller: AttachmentPickerController,
    onDismiss: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    val colors = MaterialTheme.colorScheme

    fun dismissAndRun(action: () -> Unit) {
        scope.launch {
            runCatching { sheetState.hide() }
            onDismiss()
            action()
        }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = OrcaColors.bgPanel,
        contentColor = colors.onSurface,
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp),
    ) {
        Column(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .navigationBarsPadding()
                    .padding(horizontal = OrcaSpace.lg)
                    .padding(bottom = OrcaSpace.lg),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.md),
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    text = "添加附件",
                    style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.SemiBold),
                    color = colors.onSurface,
                )
                Text(
                    text = "支持图片、代码与文档，单文件上限 4 MB，最多 16 个",
                    style = MaterialTheme.typography.bodySmall,
                    color = colors.onSurfaceVariant,
                )
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                AttachmentOptionCard(
                    title = "相册图片",
                    subtitle = "照片与截图",
                    modifier = Modifier.weight(1f),
                    onClick = { dismissAndRun(controller.pickPhotos) },
                ) {
                    AttachmentGalleryIcon(OrcaColors.accentBlue, 22.dp)
                }
                AttachmentOptionCard(
                    title = "拍摄照片",
                    subtitle = "相机直拍",
                    modifier = Modifier.weight(1f),
                    onClick = { dismissAndRun(controller.takePhoto) },
                ) {
                    AttachmentCameraIcon(OrcaColors.accentGreen, 22.dp)
                }
                AttachmentOptionCard(
                    title = "选择文件",
                    subtitle = "代码与文档",
                    modifier = Modifier.weight(1f),
                    onClick = { dismissAndRun(controller.pickFiles) },
                ) {
                    AttachmentFileIcon(OrcaColors.accentAmber, 22.dp)
                }
            }
            Spacer(Modifier.height(OrcaSpace.xs))
        }
    }
}

@Composable
private fun AttachmentOptionCard(
    title: String,
    subtitle: String,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
    icon: @Composable () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier,
        shape = RoundedCornerShape(16.dp),
        color = OrcaColors.bgRaised.copy(alpha = 0.65f),
        border = BorderStroke(1.dp, OrcaColors.borderSubtle),
    ) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 14.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Box(
                modifier =
                    Modifier
                        .size(42.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .background(OrcaColors.bgPanel),
                contentAlignment = Alignment.Center,
            ) {
                icon()
            }
            Text(
                text = title,
                style = MaterialTheme.typography.labelLarge.copy(fontWeight = FontWeight.Medium),
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
            )
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
            )
        }
    }
}

@Composable
private fun AttachmentGalleryIcon(tint: Color, size: Dp = 22.dp) {
    Canvas(Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.7.dp.toPx()
        drawRoundRect(
            color = tint,
            topLeft = Offset(w * 0.12f, h * 0.16f),
            size = Size(w * 0.76f, h * 0.68f),
            cornerRadius = CornerRadius(3.dp.toPx(), 3.dp.toPx()),
            style = Stroke(width = stroke),
        )
        drawCircle(color = tint, radius = w * 0.08f, center = Offset(w * 0.34f, h * 0.36f))
        val mountain = Path().apply {
            moveTo(w * 0.16f, h * 0.76f)
            lineTo(w * 0.44f, h * 0.50f)
            lineTo(w * 0.62f, h * 0.66f)
            lineTo(w * 0.74f, h * 0.54f)
            lineTo(w * 0.86f, h * 0.72f)
        }
        drawPath(mountain, tint, style = Stroke(stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}

@Composable
private fun AttachmentCameraIcon(tint: Color, size: Dp = 22.dp) {
    Canvas(Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.7.dp.toPx()
        val body = Path().apply {
            moveTo(w * 0.14f, h * 0.34f)
            lineTo(w * 0.30f, h * 0.34f)
            lineTo(w * 0.38f, h * 0.20f)
            lineTo(w * 0.62f, h * 0.20f)
            lineTo(w * 0.70f, h * 0.34f)
            lineTo(w * 0.86f, h * 0.34f)
            lineTo(w * 0.86f, h * 0.80f)
            lineTo(w * 0.14f, h * 0.80f)
            close()
        }
        drawPath(body, tint, style = Stroke(stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
        drawCircle(
            color = tint,
            radius = w * 0.16f,
            center = Offset(w * 0.50f, h * 0.57f),
            style = Stroke(width = stroke),
        )
    }
}

@Composable
private fun AttachmentFileIcon(tint: Color, size: Dp = 22.dp) {
    Canvas(Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.7.dp.toPx()
        val doc = Path().apply {
            moveTo(w * 0.22f, h * 0.14f)
            lineTo(w * 0.58f, h * 0.14f)
            lineTo(w * 0.78f, h * 0.34f)
            lineTo(w * 0.78f, h * 0.86f)
            lineTo(w * 0.22f, h * 0.86f)
            close()
        }
        drawPath(doc, tint, style = Stroke(stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
        val fold = Path().apply {
            moveTo(w * 0.58f, h * 0.14f)
            lineTo(w * 0.58f, h * 0.34f)
            lineTo(w * 0.78f, h * 0.34f)
        }
        drawPath(fold, tint, style = Stroke(stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
        drawLine(tint, Offset(w * 0.34f, h * 0.54f), Offset(w * 0.66f, h * 0.54f), stroke, StrokeCap.Round)
        drawLine(tint, Offset(w * 0.34f, h * 0.68f), Offset(w * 0.56f, h * 0.68f), stroke, StrokeCap.Round)
    }
}
