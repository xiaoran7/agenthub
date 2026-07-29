package com.agentplatform.mobile

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import com.agentplatform.mobile.data.SecureConfigStore
import com.agentplatform.mobile.ui.AgentPlatformApp
import com.agentplatform.mobile.ui.AgentPlatformTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            val controller = remember { AppController(SecureConfigStore(applicationContext)) }
            DisposableEffect(controller) {
                onDispose { controller.close() }
            }
            AgentPlatformTheme {
                AgentPlatformApp(controller)
            }
        }
    }
}
