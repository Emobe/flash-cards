package dev.placeholder.flashcards

import android.os.Bundle
import android.os.Environment
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    deleteOldCameraFiles()
  }

  // The WebView hands a photo taken from the page to the page and never deletes the file it made
  // (ADR 0011 decision 2). The page has its own copy by then, so files older than an hour can go.
  private fun deleteOldCameraFiles() {
    val folder = getExternalFilesDir(Environment.DIRECTORY_PICTURES) ?: return
    val cutoff = System.currentTimeMillis() - 60 * 60 * 1000
    folder.listFiles { file -> file.name.startsWith("JPEG_") && file.name.endsWith(".jpg") }
      ?.filter { it.isFile && it.lastModified() < cutoff }
      ?.forEach { it.delete() }
  }
}
