package org.rstorrent.bootstrap

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import org.junit.Rule
import org.junit.Test
import org.rstorrent.bootstrap.ui.ProductApp
import org.rstorrent.bootstrap.ui.ProductThemeMode

class ProductNavigationTest {
    @get:Rule val compose = createComposeRule()

    @Test
    fun libraryReachesSettingsHierarchyAndAddIntake() {
        compose.setContent {
            ProductApp(
                service = null,
                onSelectStorage = {},
                onBrowseTorrent = {},
                notificationsGranted = true,
                onRequestNotifications = {},
                onOpenNotificationSettings = {},
                themeMode = ProductThemeMode.LIGHT,
                dynamicColor = false,
                onThemeMode = {},
                onDynamicColor = {},
            )
        }

        compose.onNodeWithContentDescription("Add torrent").performClick()
        compose.onNodeWithText("Browse .torrent file").assertIsDisplayed()
        compose.onNodeWithText("Cancel").performClick()

        compose.onNodeWithContentDescription("More options").performClick()
        compose.onNodeWithText("Settings").performClick()
        compose.onNodeWithText("Storage").performClick()
        compose.onNodeWithText("Download folder").assertIsDisplayed()
    }
}
