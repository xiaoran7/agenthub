package com.agentplatform.mobile

import android.app.Application
import com.agentplatform.mobile.data.AppearanceStore
import com.agentplatform.mobile.data.SecureConfigStore
import com.agentplatform.mobile.data.ServerAddressStore

/**
 * Process-scoped owner of the app controller.
 *
 * The controller used to be created by `remember` inside the activity's
 * composition, so rotating the phone (or changing the system font scale)
 * destroyed it: `onDispose` cancelled every coroutine, in-flight commands
 * called back into a dead scope, and the whole credential read plus the
 * account fetch ran again. Holding it here ties its lifetime to the process,
 * which is the same lifetime its state actually has.
 */
class AgentHubApplication : Application() {
    val controller: AppController by lazy {
        AppController(
            SecureConfigStore(this),
            AppearanceStore(this),
            ServerAddressStore(this),
        )
    }
}
