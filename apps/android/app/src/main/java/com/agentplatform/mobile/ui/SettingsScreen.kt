package com.agentplatform.mobile.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.BuildConfig
import com.agentplatform.mobile.data.AccountView
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.AppearanceTheme

@Composable
fun SettingsScreen(
    controller: AppController,
    account: AccountView?,
    config: AppConfig?,
    appearance: AppearanceTheme,
    onBack: (() -> Unit)? = null,
    onOpenHosts: (() -> Unit)? = null,
) {
    var showThemePicker by rememberSaveable { mutableStateOf(false) }
    var confirmLogout by rememberSaveable { mutableStateOf(false) }
    val pluginActions = remember(controller) { SettingsPluginActions(controller) }
    val hosts = controller.state.snapshot.hosts
    val visibleHosts = hosts.count { !it.revoked }
    val onlineHosts = hosts.count { !it.revoked && it.online }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding()
            .navigationBarsPadding()
            .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal)),
    ) {
        SettingsHeader(onBack)

        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f),
            contentPadding = PaddingValues(start = 20.dp, end = 20.dp, bottom = 24.dp),
        ) {
            item(key = "profile") { SettingsProfile(account) }
            item(key = "account") {
                OrcaSectionHeader("账号与设备")
                OrcaListGroup {
                    OrcaRow(
                        title = "设备管理",
                        modifier = Modifier.heightIn(min = 56.dp),
                        leading = { OrcaMonitorIcon(MaterialTheme.colorScheme.onSurface, 20.dp) },
                        trailing = {
                            SettingsValue(if (visibleHosts == 0) "暂无设备" else "${onlineHosts}/${visibleHosts} 在线")
                            if (onOpenHosts != null) {
                                OrcaChevronRight(MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        },
                        onClick = onOpenHosts,
                    )
                    OrcaRowDivider()
                    OrcaRow(
                        title = "退出登录",
                        titleColor = MaterialTheme.colorScheme.error,
                        modifier = Modifier.heightIn(min = 56.dp),
                        trailing = { OrcaChevronRight(MaterialTheme.colorScheme.onSurfaceVariant) },
                        onClick = { confirmLogout = true },
                    )
                }
            }
            item(key = "appearance") {
                OrcaSectionHeader("外观")
                OrcaListGroup {
                    OrcaRow(
                        title = "主题",
                        modifier = Modifier.heightIn(min = 56.dp),
                        leading = { AppearanceSwatch(appearance) },
                        trailing = {
                            SettingsValue(appearance.label)
                            OrcaChevronDown(MaterialTheme.colorScheme.onSurfaceVariant)
                        },
                        onClick = { showThemePicker = true },
                    )
                }
            }
            item(key = "gateway") { SettingsGateway(controller, config) }
            item(key = "plugins") { SettingsPlugins(controller, pluginActions) }
            item(key = "privacy") { SettingsPrivacy() }
            item(key = "version") {
                Text(
                    text = "AgentHub Android v${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(top = 28.dp, bottom = 8.dp),
                )
            }
        }
    }

    if (showThemePicker) {
        SettingsThemePicker(
            appearance = appearance,
            onSelect = {
                controller.setAppearance(it)
                showThemePicker = false
            },
            onDismiss = { showThemePicker = false },
        )
    }

    if (confirmLogout) {
        AlertDialog(
            onDismissRequest = { confirmLogout = false },
            title = { Text("退出登录？") },
            text = { Text("本机保存的登录凭据将被清除。") },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmLogout = false
                        controller.logout()
                    },
                ) { Text("退出登录", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { confirmLogout = false }) { Text("取消") }
            },
        )
    }
}

@Composable
private fun SettingsHeader(onBack: (() -> Unit)?) {
    Box(
        modifier = Modifier.fillMaxWidth().heightIn(min = 64.dp).padding(horizontal = 20.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = "设置",
            style = MaterialTheme.typography.titleLarge,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.semantics { heading() },
        )
        if (onBack != null) {
            Surface(
                onClick = onBack,
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .size(48.dp)
                    .semantics { contentDescription = "返回" },
                shape = CircleShape,
                color = MaterialTheme.colorScheme.surface,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
            ) {
                Box(contentAlignment = Alignment.Center) {
                    OrcaBackChevron(MaterialTheme.colorScheme.onSurface, 22.dp)
                }
            }
        }
    }
}

@Composable
private fun SettingsProfile(account: AccountView?) {
    val displayName = account?.displayName.orEmpty().trim()
    val email = account?.email.orEmpty().trim()
    val identity = displayName.ifBlank { email }
    val plan = account?.plan?.trim()?.takeIf { it.isNotEmpty() }
    val workspace = account?.workspaceName?.trim()?.takeIf { it.isNotEmpty() }

    Column(
        modifier = Modifier.fillMaxWidth().padding(top = 16.dp, bottom = 10.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(
            modifier = Modifier
                .padding(bottom = 4.dp)
                .size(76.dp)
                .background(OrcaColors.bgRaised, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            if (identity.isNotEmpty()) {
                val initial = String(Character.toChars(identity.codePointAt(0))).uppercase()
                Text(initial, style = MaterialTheme.typography.headlineLarge, color = OrcaColors.accentBlue)
            } else {
                OrcaLogo(27.dp, MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        Text(
            text = identity.ifEmpty { "账号资料暂不可用" },
            style = MaterialTheme.typography.titleLarge,
            color = MaterialTheme.colorScheme.onSurface,
            textAlign = TextAlign.Center,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        if (email.isNotBlank() && email != identity) {
            Text(
                text = email,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        val accountDetails = listOfNotNull(workspace, plan).joinToString(" · ")
        if (accountDetails.isNotEmpty()) {
            Surface(shape = CircleShape, color = OrcaColors.bgRaised, modifier = Modifier.widthIn(max = 320.dp)) {
                Text(
                    text = accountDetails,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp),
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsThemePicker(
    appearance: AppearanceTheme,
    onSelect: (AppearanceTheme) -> Unit,
    onDismiss: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = MaterialTheme.colorScheme.surface,
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp),
    ) {
        Text(
            text = "主题",
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(start = 24.dp, end = 24.dp, bottom = 8.dp).semantics { heading() },
        )
        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f, fill = false).selectableGroup(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 16.dp),
        ) {
            items(AppearanceTheme.entries.size, key = { AppearanceTheme.entries[it].wire }) { index ->
                val theme = AppearanceTheme.entries[index]
                val selected = theme == appearance
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 60.dp)
                        .clip(RoundedCornerShape(12.dp))
                        .selectable(selected = selected, role = Role.RadioButton, onClick = { scope.launch { runCatching { sheetState.hide() }; onSelect(theme) } })
                        .padding(horizontal = 8.dp, vertical = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    AppearanceSwatch(theme)
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(theme.label, style = MaterialTheme.typography.bodyLarge)
                        Text(
                            theme.blurb,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    RadioButton(selected = selected, onClick = null)
                }
            }
        }
    }
}

@Composable
private fun AppearanceSwatch(theme: AppearanceTheme) {
    val colors = theme.palette
    val fill = if (theme == AppearanceTheme.SYSTEM) {
        Modifier.background(
            brush = Brush.linearGradient(
                listOf(
                    AppearanceTheme.MINT_LIGHT.palette.bgBase,
                    AppearanceTheme.FOREST_DARK.palette.bgBase,
                ),
            ),
            shape = CircleShape,
        )
    } else {
        Modifier.background(colors.bgBase, CircleShape)
    }
    Box(
        modifier = Modifier
            .size(24.dp)
            .then(fill)
            .border(1.dp, MaterialTheme.colorScheme.outline, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(8.dp).background(colors.accentBlue, CircleShape))
    }
}

@Composable
private fun SettingsValue(value: String) {
    Text(
        text = value,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
}

private fun credentialLabel(config: AppConfig): String =
    when {
        config.sessionCookie.isNotBlank() -> "账号会话"
        config.accessToken.isNotBlank() -> "访问令牌"
        config.developmentUserId.isNotBlank() -> "本机开发身份"
        else -> "未配置"
    }

@Composable
private fun SettingsGateway(controller: AppController, config: AppConfig?) {
    var editing by rememberSaveable { mutableStateOf(false) }
    var draft by rememberSaveable { mutableStateOf(config?.gatewayBase.orEmpty()) }
    var saveError by rememberSaveable { mutableStateOf<String?>(null) }

    OrcaSectionHeader("连接")
    OrcaListGroup {
        OrcaRow(
            title = "服务端地址",
            modifier = Modifier.semantics { stateDescription = if (editing) "已展开" else "已收起" },
            subtitle = config?.gatewayBase ?: "未配置",
            meta = config?.let { "登录方式 · ${credentialLabel(it)}" },
            leading = { OrcaTerminalIcon(MaterialTheme.colorScheme.onSurface, 20.dp) },
            trailing = {
                OrcaChevronDown(MaterialTheme.colorScheme.onSurfaceVariant, expanded = editing)
            },
            onClick = {
                if (!editing) draft = config?.gatewayBase.orEmpty()
                saveError = null
                editing = !editing
            },
        )
        if (editing) {
            OrcaRowDivider()
            Column(
                modifier = Modifier.fillMaxWidth().padding(OrcaSpace.rowH),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                GatewayField(
                    value = draft,
                    onValueChange = { draft = it },
                    label = "Gateway 地址",
                    placeholder = "https://gateway.example.com",
                )
                Text(
                    text = "切换服务器后，需使用新服务器的账号重新登录。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                saveError?.let { OrcaInlineError(it) }
                Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm)) {
                    OrcaSecondaryButton("重置", Modifier.weight(1f)) {
                        draft = config?.gatewayBase.orEmpty()
                        saveError = null
                    }
                    OrcaPrimaryButton("保存", Modifier.weight(1f), enabled = draft.isNotBlank()) {
                        val next = (config ?: AppConfig(gatewayBase = draft)).copy(gatewayBase = draft)
                        saveError = controller.saveConfig(next)
                        if (saveError == null) editing = false
                    }
                }
            }
        }
    }
}

@Composable
private fun SettingsPrivacy() {
    var expanded by rememberSaveable { mutableStateOf(false) }

    OrcaSectionHeader("隐私")
    OrcaListGroup {
        OrcaRow(
            title = "本机存储与安全",
            modifier = Modifier.heightIn(min = 56.dp).semantics {
                stateDescription = if (expanded) "已展开" else "已收起"
            },
            trailing = {
                OrcaChevronDown(MaterialTheme.colorScheme.onSurfaceVariant, expanded = expanded)
            },
            onClick = { expanded = !expanded },
        )
        if (expanded) {
            OrcaRowDivider()
            Column(
                modifier = Modifier.fillMaxWidth().padding(OrcaSpace.rowH),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.md),
            ) {
                SettingsInfo("凭据加密存放", "Android Keystore 加密保存账号会话与访问令牌。")
                SettingsInfo("会话保存在内存", "打开会话时读取内容，退出会话后释放本机副本。")
                SettingsInfo("任务在电脑上执行", "手机下发指令与审批，电脑执行命令并访问项目文件。")
            }
        }
    }
}

@Composable
private fun SettingsInfo(title: String, body: String) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(title, style = MaterialTheme.typography.titleSmall)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
