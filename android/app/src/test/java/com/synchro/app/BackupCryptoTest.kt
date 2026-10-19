package com.synchro.app

import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.ByteArrayInputStream

class BackupCryptoTest {
    @Test fun matchesRustCiphertextGoldenVector() {
        val key=BackupCrypto.fromHex("ab".repeat(32))
        val params=BackupCrypto.Parameters(ByteArray(16) { 1 },ByteArray(8) { 2 })
        val plain="Synchro cross-platform test".toByteArray(Charsets.UTF_8)
        assertEquals(83L, BackupCrypto.cipherSize(plain.size.toLong()))
        val result=BackupCrypto.shaStream(
            BackupCrypto.stream(ByteArrayInputStream(plain),plain.size.toLong(),key,params))
        assertEquals(
            "38173b51c9a6e35acd41459469e4f15ba31e4c0bcba033a5c93108f77d55d578",
            result)
    }
    @Test fun rejectsMalformedMasterKey() {
        try {
            BackupCrypto.validateHex("abc")
            throw AssertionError("Invalid master key was accepted")
        } catch (_: IllegalArgumentException) { }
    }
}
