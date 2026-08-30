package org.rstorrent.bootstrap

import android.Manifest
import android.os.Build
import androidx.test.rule.GrantPermissionRule
import org.junit.rules.TestRule
import org.junit.runner.Description
import org.junit.runners.model.Statement

class NotificationPermissionRule : TestRule {
    private val permission =
        GrantPermissionRule.grant(Manifest.permission.POST_NOTIFICATIONS)

    override fun apply(
        base: Statement,
        description: Description,
    ): Statement =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            permission.apply(base, description)
        } else {
            base
        }
}
