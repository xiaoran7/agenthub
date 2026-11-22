package com.agentplatform.mobile.ui

import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.provider.OpenableColumns

// =========================================================================
// Storage Access Framework helpers
//
// The document picker hands back a content URI, not a file: the name and size
// live in the provider's own metadata table, and reading the bytes goes through
// a stream the provider controls. Both are queried defensively because a
// provider is free to answer with nothing at all — a cloud-backed picker with
// the file not yet downloaded returns a null size, and treating that as zero
// would silently accept an oversized file.
//
// A blank name is reported as a blank string, not as null: the two mean
// different things to the caller. Null is "the provider would not tell us",
// which is a file to skip and say so about; "" is "the provider told us the
// file has no name", which the reader has its own placeholder for. Collapsing
// them would drop a file the person picked without a word of explanation.
// =========================================================================

internal fun Context.queryDisplayName(uri: Uri): String? =
    runCatching {
        contentResolver
            .query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { cursor: Cursor ->
                if (!cursor.moveToFirst()) return@use null
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index < 0) null else cursor.getString(index).orEmpty()
            }
    }.getOrNull()

internal fun Context.querySize(uri: Uri): Long? =
    runCatching {
        contentResolver
            .query(uri, arrayOf(OpenableColumns.SIZE), null, null, null)
            ?.use { cursor: Cursor ->
                if (!cursor.moveToFirst()) return@use null
                val index = cursor.getColumnIndex(OpenableColumns.SIZE)
                if (index < 0 || cursor.isNull(index)) null else cursor.getLong(index)
            }
    }.getOrNull()
