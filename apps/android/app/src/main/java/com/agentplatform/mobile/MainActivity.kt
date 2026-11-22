package com.agentplatform.mobile

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import com.agentplatform.mobile.ui.AgentPlatformApp
import com.agentplatform.mobile.ui.AgentPlatformTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val controller = (application as AgentHubApplication).controller
        setContent {
            // The palette comes from the controller so a change in Settings
            // repaints the whole app on the next frame, without a restart.
            AgentPlatformTheme(appearance = controller.state.appearance) {
                AgentPlatformApp(controller)
            }
        }
    }
}
