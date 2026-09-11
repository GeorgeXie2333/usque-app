package io.github.georgexie2333.usque

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AndroidLocaleControllerTest {
    @Test
    fun `catalog ids map to the Flutter locale variants`() {
        assertNull(AndroidLocaleController.languageTagForCatalog("system"))
        assertEquals("zh-CN", AndroidLocaleController.languageTagForCatalog("zh_CN"))
        assertEquals("zh-HK", AndroidLocaleController.languageTagForCatalog("zh_HK"))
        assertEquals("zh-TW", AndroidLocaleController.languageTagForCatalog("zh_TW"))
        assertEquals("pt-BR", AndroidLocaleController.languageTagForCatalog("pt"))
        assertEquals("id", AndroidLocaleController.languageTagForCatalog("id"))
        assertNull(AndroidLocaleController.languageTagForCatalog("unsupported"))
    }
}
