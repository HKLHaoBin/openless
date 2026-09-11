package com.openless.app

import android.os.Handler
import android.os.Looper
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.UserNotAuthenticatedException
import androidx.annotation.Keep
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.InvalidKeyException
import java.security.KeyStore
import java.security.KeyStoreException
import java.security.UnrecoverableKeyException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import javax.crypto.AEADBadTagException
import javax.crypto.BadPaddingException
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

internal const val CREDENTIAL_STATUS_OK: Byte = 0
internal const val CREDENTIAL_STATUS_KEY_MISSING: Byte = 1
internal const val CREDENTIAL_STATUS_AUTHENTICATION_FAILED: Byte = 2
internal const val CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE: Byte = 3
internal const val CREDENTIAL_STATUS_MALFORMED: Byte = 4

private fun credentialResponse(status: Byte, payload: ByteArray = byteArrayOf()): ByteArray {
    return byteArrayOf(status) + payload
}

private fun diagnosticResponse(status: Byte, error: Throwable): ByteArray {
    val name = buildString {
        append(error.javaClass.simpleName.take(40))
        error.cause?.javaClass?.simpleName?.let { cause ->
            append('/')
            append(cause.take(40))
        }
        keystoreNumericCode(error)?.let { code ->
            append(':')
            append(code)
        }
        append(':')
        append(if (Looper.myLooper() == Looper.getMainLooper()) "main" else "bg")
    }
    return credentialResponse(status, name.toByteArray(Charsets.UTF_8))
}

private fun keystoreNumericCode(error: Throwable): Int? {
    var current: Throwable? = error
    while (current != null) {
        try {
            for (methodName in arrayOf("getNumericErrorCode", "getErrorCode")) {
                val method =
                    current.javaClass.methods.firstOrNull { it.name == methodName && it.parameterCount == 0 }
                        ?: continue
                when (val value = method.invoke(current)) {
                    is Int -> return value
                }
            }
        } catch (_: Throwable) {}
        current = current.cause
    }
    return null
}

internal fun credentialStatusForKeyLoadFailure(error: GeneralSecurityException): Byte {
    return when (error) {
        is KeyPermanentlyInvalidatedException -> CREDENTIAL_STATUS_KEY_MISSING
        else -> CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE
    }
}

internal fun credentialStatusForCipherKeyFailure(error: InvalidKeyException): Byte {
    return when (error) {
        is UserNotAuthenticatedException -> CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE
        // Cipher cannot use this key, so the envelope cannot be recovered.
        else -> CREDENTIAL_STATUS_KEY_MISSING
    }
}

/** AndroidKeyStore owner with fixed, secret-free status responses for JNI. */
internal class AndroidKeystoreCredentialVault(private val alias: String) {
    @Synchronized
    fun seal(plaintext: ByteArray, aad: ByteArray): ByteArray {
        val first = sealOnce(plaintext, aad, recreate = false)
        if (first.first() == CREDENTIAL_STATUS_OK || first.first() == CREDENTIAL_STATUS_MALFORMED) {
            return first
        }
        return sealOnce(plaintext, aad, recreate = true)
    }

    private fun sealOnce(plaintext: ByteArray, aad: ByteArray, recreate: Boolean): ByteArray {
        return try {
            if (recreate) {
                deleteEntryQuiet()
            }
            val key = if (recreate) createKey() else getOrCreateKey()
            credentialResponse(CREDENTIAL_STATUS_OK, OpenLessCredentialCipher.seal(key, plaintext, aad))
        } catch (error: KeyPermanentlyInvalidatedException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: UnrecoverableKeyException) {
            // Keystore2 wraps backend-busy and other provider failures in this
            // broad JCA exception too. Only an absent alias or the explicit
            // permanent-invalidated exception is safe to treat as data loss.
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: InvalidKeyException) {
            diagnosticResponse(credentialStatusForCipherKeyFailure(error), error)
        } catch (_: IllegalArgumentException) {
            credentialResponse(CREDENTIAL_STATUS_MALFORMED)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    @Synchronized
    fun open(packet: ByteArray, aad: ByteArray): ByteArray {
        return try {
            val key = existingKey() ?: return credentialResponse(CREDENTIAL_STATUS_KEY_MISSING)
            credentialResponse(
                CREDENTIAL_STATUS_OK,
                OpenLessCredentialCipher.open(key, packet, aad),
            )
        } catch (error: KeyPermanentlyInvalidatedException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: UnrecoverableKeyException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: InvalidKeyException) {
            diagnosticResponse(credentialStatusForCipherKeyFailure(error), error)
        } catch (_: AEADBadTagException) {
            credentialResponse(CREDENTIAL_STATUS_AUTHENTICATION_FAILED)
        } catch (_: BadPaddingException) {
            credentialResponse(CREDENTIAL_STATUS_AUTHENTICATION_FAILED)
        } catch (_: IllegalArgumentException) {
            credentialResponse(CREDENTIAL_STATUS_MALFORMED)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    @Synchronized
    fun deleteKey(): ByteArray {
        return try {
            val keyStore = loadKeyStore()
            if (keyStore.containsAlias(alias)) {
                keyStore.deleteEntry(alias)
            }
            credentialResponse(CREDENTIAL_STATUS_OK)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    @Synchronized
    fun keyExists(): ByteArray {
        return try {
            credentialResponse(
                CREDENTIAL_STATUS_OK,
                byteArrayOf(if (loadKeyStore().containsAlias(alias)) 1 else 0),
            )
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    @Synchronized
    fun ensureKey(): ByteArray {
        return try {
            getOrCreateKey()
            credentialResponse(CREDENTIAL_STATUS_OK)
        } catch (error: KeyPermanentlyInvalidatedException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: UnrecoverableKeyException) {
            diagnosticResponse(credentialStatusForKeyLoadFailure(error), error)
        } catch (error: GeneralSecurityException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: IOException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        } catch (error: RuntimeException) {
            diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, error)
        }
    }

    @Throws(GeneralSecurityException::class, IOException::class)
    private fun existingKey(): SecretKey? {
        val keyStore = loadKeyStore()
        if (!keyStore.containsAlias(alias)) {
            return null
        }
        return keyStore.getKey(alias, null) as? SecretKey
    }

    @Throws(GeneralSecurityException::class, IOException::class)
    private fun getOrCreateKey(): SecretKey {
        existingKey()?.let {
            return it
        }
        return createKey()
    }

    @Throws(GeneralSecurityException::class, IOException::class)
    private fun createKey(): SecretKey {
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(
                    alias,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build()
        )
        return generator.generateKey()
    }

    private fun deleteEntryQuiet() {
        try {
            val keyStore = loadKeyStore()
            if (keyStore.containsAlias(alias)) {
                keyStore.deleteEntry(alias)
            }
        } catch (_: Exception) {}
    }

    @Throws(KeyStoreException::class, IOException::class, GeneralSecurityException::class)
    private fun loadKeyStore(): KeyStore {
        return KeyStore.getInstance(KEYSTORE_PROVIDER).apply { load(null) }
    }

    private companion object {
        const val KEYSTORE_PROVIDER = "AndroidKeyStore"
    }
}

@Keep
object OpenLessCredentialVault {
    // v2 alias on this HyperOS device became unusable (InvalidKeyException /
    // ProviderException). v3 is a fresh Keystore2 slot after envelope wipe.
    private const val KEY_ALIAS = "com.openless.app.credentials.v3"
    private const val MIGRATION_MARKER_ALIAS = "com.openless.app.credentials.v3.migrated"
    private val backend = AndroidKeystoreCredentialVault(KEY_ALIAS)
    private val migrationMarker = AndroidKeystoreCredentialVault(MIGRATION_MARKER_ALIAS)

    @JvmStatic
    fun seal(plaintext: ByteArray, aad: ByteArray): ByteArray = runOnMain { backend.seal(plaintext, aad) }

    @JvmStatic fun open(packet: ByteArray, aad: ByteArray): ByteArray = runOnMain { backend.open(packet, aad) }

    @JvmStatic fun deleteKey(): ByteArray = runOnMain { backend.deleteKey() }

    @JvmStatic fun migrationComplete(): ByteArray = runOnMain { migrationMarker.keyExists() }

    @JvmStatic fun markMigrationComplete(): ByteArray = runOnMain { migrationMarker.ensureKey() }

    private fun runOnMain(block: () -> ByteArray): ByteArray {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            return block()
        }
        val result = arrayOfNulls<ByteArray>(1)
        val error = arrayOfNulls<Throwable>(1)
        val latch = CountDownLatch(1)
        Handler(Looper.getMainLooper()).post {
            try {
                result[0] = block()
            } catch (thrown: Throwable) {
                error[0] = thrown
            } finally {
                latch.countDown()
            }
        }
        if (!latch.await(8, TimeUnit.SECONDS)) {
            return diagnosticResponse(
                CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE,
                TimeoutException("keystore-main-timeout"),
            )
        }
        error[0]?.let { thrown ->
            return diagnosticResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE, thrown)
        }
        return result[0] ?: credentialResponse(CREDENTIAL_STATUS_TEMPORARILY_UNAVAILABLE)
    }
}
