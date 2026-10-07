package io.github.emobe.flashcards

import android.app.Activity
import androidx.core.view.WindowCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class BarStyleArgs {
  /** True when the page is light, so the bar icons must be dark. */
  var lightBars: Boolean = false
}

/**
 * Sets the status bar and navigation bar icon colour to match the app's theme (ADR 0010).
 *
 * Called from Rust only, behind the token-checked `set_system_theme` command. It is registered with
 * no JS permissions, so no webview (card frames included) can invoke it (ADR 0005).
 */
@app.tauri.annotation.TauriPlugin
class AppearancePlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun setBarStyle(invoke: Invoke) {
    val args = invoke.parseArgs(BarStyleArgs::class.java)
    activity.runOnUiThread {
      val controller = WindowCompat.getInsetsController(activity.window, activity.window.decorView)
      controller.isAppearanceLightStatusBars = args.lightBars
      controller.isAppearanceLightNavigationBars = args.lightBars
      invoke.resolve()
    }
  }
}
