package com.agentplatform.mobile.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DrawerState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.data.AccountView

@Composable
internal fun MobileNavigationSheet(
    drawerState: DrawerState,
    width: Dp,
    current: String,
    waitingCount: Int,
    deviceCount: Int,
    account: AccountView?,
    onSelect: (String) -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    ModalDrawerSheet(
        drawerState = drawerState,
        modifier = Modifier.width(width).fillMaxHeight(),
        drawerShape = RectangleShape,
        drawerContainerColor = colors.background,
        drawerContentColor = colors.onBackground,
        drawerTonalElevation = 0.dp,
    ) {
        Column(Modifier.fillMaxWidth().fillMaxHeight().padding(horizontal = OrcaSpace.lg)) {
            Row(
                modifier = Modifier.padding(top = OrcaSpace.lg, bottom = OrcaSpace.xl, start = OrcaSpace.sm),
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.md),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OrcaLogo(size = 24.dp)
                Text("AgentHub", style = MaterialTheme.typography.headlineMedium)
            }
            Column(
                modifier = Modifier.weight(1f).verticalScroll(rememberScrollState()),
            ) {
                OrcaRow(
                    title = "会话",
                    modifier = Modifier.semantics { selected = current == DEST_SESSIONS },
                    leading = { OrcaQueueIcon(colors.onSurface, 22.dp) },
                    trailing = {
                        if (waitingCount > 0) {
                            Text(
                                "${waitingCount} 项待处理",
                                style = MaterialTheme.typography.bodySmall,
                                color = colors.primary,
                            )
                        }
                    },
                    onClick = { onSelect(DEST_SESSIONS) },
                )
                OrcaRowDivider(startInset = 0.dp)
                OrcaRow(
                    title = "电脑管理",
                    modifier = Modifier.semantics { selected = current == DEST_HOSTS },
                    leading = { OrcaMonitorIcon(colors.onSurface, 22.dp) },
                    trailing = {
                        Text(
                            "$deviceCount 台",
                            style = MaterialTheme.typography.bodySmall,
                            color = colors.onSurfaceVariant,
                        )
                    },
                    onClick = { onSelect(DEST_HOSTS) },
                )
            }
            OrcaRowDivider(startInset = 0.dp)
            MobileNavigationAccount(account) { onSelect(DEST_SETTINGS) }
        }
    }
}

@Composable
private fun MobileNavigationAccount(account: AccountView?, onSettings: () -> Unit) {
    val displayName = account?.displayName.orEmpty().ifBlank { account?.email.orEmpty() }
    val colors = MaterialTheme.colorScheme
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.lg),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.md),
    ) {
        Surface(
            modifier = Modifier.size(OrcaSize.iconButton),
            shape = CircleShape,
            color = colors.surfaceVariant,
        ) {
            Box(contentAlignment = Alignment.Center) {
                if (displayName.isBlank()) {
                    OrcaLogo(size = 18.dp)
                } else {
                    Text(
                        displayName.take(1).uppercase(),
                        style = MaterialTheme.typography.titleMedium,
                        color = colors.onSurfaceVariant,
                    )
                }
            }
        }
        Column(Modifier.weight(1f)) {
            Text(
                displayName.ifBlank { "账户设置" },
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.titleMedium,
            )
            account?.workspaceName?.takeIf { it.isNotBlank() }?.let { workspace ->
                Text(
                    workspace,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodySmall,
                    color = colors.onSurfaceVariant,
                )
            }
        }
        MobileRoundButton(label = "设置", onClick = onSettings) {
            OrcaSettingsIcon(colors.onSurface, 22.dp)
        }
    }
}

@Composable
internal fun MobileRoundButton(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: @Composable () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.size(OrcaSize.iconButton).semantics { contentDescription = label },
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        shadowElevation = 2.dp,
    ) {
        Box(contentAlignment = Alignment.Center) { icon() }
    }
}
