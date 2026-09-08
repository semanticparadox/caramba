package com.caramba.caramba_vpn

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import android.util.Base64
import org.json.JSONObject
import java.math.BigInteger
import java.security.KeyFactory
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.PublicKey
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import java.security.spec.PKCS8EncodedKeySpec
import java.security.spec.X509EncodedKeySpec
import javax.crypto.KeyAgreement

// CarambaDeviceKeys is the Android holder of the CSM/1 device identity
// (02-SPEC.md 9.4, 12.2 and 03-WIRE.md 13.6).
//
// The signing key MUST live in StrongBox or the TEE, and neither is reachable
// from Go or Dart: a Go implementation would by definition put the key in a
// file, which is the software tier. So the key lives here, behind the
// AndroidKeyStore, and the Go core calls back into this class through
// mobile.DeviceKeyBridge for every signature and every key agreement.
//
// The tier is reported HONESTLY. A device with no StrongBox reports 2 (TEE); a
// device where the keystore itself is unusable falls back to an in-process key
// and reports 3. It never claims hardware it does not have: the operator makes
// decisions about a device from that number.
//
// Two keys, not one, because 03-WIRE.md 13.8 registers both at once and they
// have different purposes: PURPOSE_SIGN for the write proof, PURPOSE_AGREE_KEY
// for unsealing the 0x06 directive. The reported tier is the WEAKER of the two,
// because a device is only as protected as its softest half.
//
// CODE IDENTIFIERS stay 'caramba' (the user-facing brand is 'exarobot').
internal class CarambaDeviceKeys(private val context: Context) {

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val CURVE = "secp256r1"
        const val SIGN_ALIAS = "caramba.csm.device.sign"
        const val AGREE_ALIAS_PREFIX = "caramba.csm.device.agree."

        const val TIER_SECURE_ENCLAVE = 1
        const val TIER_STRONGBOX_OR_TEE = 2
        const val TIER_SOFTWARE = 3

        // Software-fallback material and the agreement-key generation live in the
        // seam prefs. Tier 3 is exactly what "a private key in app storage" means,
        // and it is reported as such.
        const val PREF_SW_SIGN = "csm.device.sw.sign"
        const val PREF_SW_AGREE = "csm.device.sw.agree"
        const val PREF_GENERATION = "csm.device.generation"

        // Own prefs file, excluded from Auto Backup and device transfer by
        // res/xml/caramba_backup_rules.xml and
        // res/xml/caramba_data_extraction_rules.xml in the host app.
        const val DEVICE_PREFS = "caramba_csm_device"

        // The one construction the device signing key ever signs
        // (03-WIRE.md 13.6). Checked on the way in, see checkWriteProofPreImage.
        const val WRITE_PROOF_LABEL = "csm1-write"
        const val PATH_PREFERENCES = "/api/v2/app/preferences"
        val CANONICAL_PATHS = setOf(
            PATH_PREFERENCES,
            "/api/v2/app/csm/enroll/code",
            "/api/v2/app/csm/enroll/device",
        )

        // The machine reason for "this device holds no agreement key of that
        // generation", which is seal step 5 and nothing else.
        const val CODE_NO_AGREEMENT_GENERATION = "no_agreement_generation"

        // DER SubjectPublicKeyInfo prefix for an uncompressed P-256 public key:
        // SEQUENCE { SEQUENCE { id-ecPublicKey, prime256v1 }, BIT STRING }.
        val SPKI_PREFIX = byteArrayOf(
            0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86.toByte(), 0x48, 0xce.toByte(),
            0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86.toByte(), 0x48, 0xce.toByte(), 0x3d,
            0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
        )

        // Order of the P-256 group and its halfway point. `s` above n/2 must be
        // flipped, and a verifier must reject a high `s` rather than normalize it,
        // so the flip happens here, once, on the way out.
        val N: BigInteger = BigInteger(
            "FFFFFFFF00000000FFFFFFFFFFFFFFFFBCE6FAADA7179E84F3B9CAC2FC632551", 16,
        )
        val HALF_N: BigInteger = N.shiftRight(1)
    }

    /**
     * The device identity lives in its OWN prefs file, not in the shared seam
     * prefs.
     *
     * Two reasons, both about blast radius. The seam file carries the panel URL,
     * the subscription id and the access token, and it is written from several
     * places; a private key that shares that file inherits every future decision
     * about it, including a backup rule someone widens later. And the backup
     * exclusion in the app manifest names a FILE: a key sitting in the seam file
     * could not be excluded without excluding the seam too.
     */
    private val prefs by lazy {
        context.getSharedPreferences(DEVICE_PREFS, Context.MODE_PRIVATE)
    }

    /** The old home of the software material, read once for migration. */
    private val legacyPrefs by lazy {
        context.getSharedPreferences(CarambaVpnKeys.PREFS, Context.MODE_PRIVATE)
    }

    private var cached: Identity? = null

    /** The established device identity: both public keys and the honest tier. */
    internal data class Identity(
        val signingSpki: ByteArray,
        val agreementPoint: ByteArray,
        val tier: Int,
        val generation: Int,
    )

    // MARK: bridge surface (the JSON shapes of ABI v3)

    /**
     * `{"purpose":"sign"|"agree","require_hardware":bool}` ->
     * `{"spki_b64","agree_pub_b64","tier","generation"}`.
     *
     * Idempotent: the identity is established once and survives restarts. A new
     * identity per launch would mean a new `dtp`, and therefore a new device in
     * the operator's list after every start of the app.
     */
    @Synchronized
    fun keygen(@Suppress("UNUSED_PARAMETER") requestJson: String): String {
        val id = identity()
        return JSONObject()
            .put("spki_b64", b64(id.signingSpki))
            .put("agree_pub_b64", b64(id.agreementPoint))
            .put("dtp_hex", dtpHex(id.signingSpki))
            .put("tier", id.tier)
            .put("generation", id.generation)
            .toString()
    }

    /**
     * `{"message_b64"}` -> `{"sig_b64"}`, 64 bytes `r || s` with low `s`.
     *
     * The MESSAGE is signed, never a pre-computed digest: `SHA256withECDSA`
     * hashes it itself, which is what 03-WIRE.md 13.6 fixes. A signer that
     * pre-hashed and then signed the digest as a message would build the right
     * pre-image and still produce a signature the panel rejects.
     */
    @Synchronized
    fun sign(requestJson: String): String {
        val message = Base64.decode(JSONObject(requestJson).optString("message_b64", ""), Base64.DEFAULT)
        if (message.isEmpty()) {
            return JSONObject().put("error", "empty message is not signed").toString()
        }
        // The one construction this key ever signs (03-WIRE.md 13.6), checked
        // here rather than assumed. StrongBox makes the key non-extractable,
        // and a holder that signs arbitrary bytes stands in for extraction:
        // anything running in this process (a third-party Flutter package, a
        // second isolate, injected code) would otherwise get a signature over
        // an enrolment body aimed at a hostile origin, over a rekey proof, or
        // over a preferences write on another canonical path.
        checkWriteProofPreImage(message)?.let {
            return JSONObject().put("error", it).toString()
        }
        identity()
        val signer = Signature.getInstance("SHA256withECDSA")
        signer.initSign(signingPrivateKey())
        signer.update(message)
        val der = signer.sign()
        val raw = derToRawSignature(der)
            ?: return JSONObject().put("error", "the platform returned a signature that is not ASN.1 ECDSA").toString()
        return JSONObject().put("sig_b64", b64(raw)).toString()
    }

    /**
     * `{"rkv","peer_pub_b64"}` -> `{"shared_b64","own_pub_b64"}`.
     *
     * `own_pub_b64` travels with the secret because DHKEM's `kem_context`
     * carries the recipient's own public key, and only the key holder knows it.
     */
    @Synchronized
    fun agree(requestJson: String): String {
        val req = JSONObject(requestJson)
        val rkv = req.optInt("rkv", 0)
        val peerBytes = Base64.decode(req.optString("peer_pub_b64", ""), Base64.DEFAULT)
        if (peerBytes.size != 65 || peerBytes[0] != 0x04.toByte()) {
            return JSONObject().put("error", "peer key is not an uncompressed P-256 point").toString()
        }
        val id = identity()
        val generation = if (rkv == 0) id.generation else rkv
        val priv = agreementPrivateKey(generation)
            ?: return JSONObject()
                .put("error", "this device holds no agreement key of generation $generation")
                // The MACHINE reason, not just the prose. Seal step 5 tells the
                // client to rekey and re-request (02-SPEC.md 10.3); step 6 tells
                // it nothing of the kind, and a bridge that answers both with
                // free text makes the core burn a key generation on every
                // corrupted enc.
                .put("code", CODE_NO_AGREEMENT_GENERATION)
                .toString()
        val peer = publicKeyFromPoint(peerBytes)
            ?: return JSONObject().put("error", "peer key does not decode").toString()
        val ka = KeyAgreement.getInstance("ECDH")
        ka.init(priv)
        ka.doPhase(peer, true)
        val shared = ka.generateSecret()
        val own = agreementPoint(generation)
            ?: return JSONObject().put("error", "the agreement public key is missing").toString()
        return JSONObject()
            .put("shared_b64", b64(shared))
            .put("own_pub_b64", b64(own))
            .toString()
    }

    /**
     * Returns a reason when [message] is not the CSM/1 write-proof pre-image of
     * 03-WIRE.md 13.6, and null when it is:
     *
     *   "csm1-write" || 0x00 || method || 0x00 || canonicalPath || 0x00 ||
     *   sha256(body)
     *
     * Read from the END rather than split on the separator: the last 32 bytes
     * are a digest and a zero byte inside it is perfectly legal.
     */
    private fun checkWriteProofPreImage(message: ByteArray): String? {
        val head = WRITE_PROOF_LABEL.toByteArray(Charsets.US_ASCII) + byteArrayOf(0)
        if (message.size < head.size + 3 + 32) {
            return "message is not a CSM/1 write proof pre-image: too short"
        }
        for (i in head.indices) {
            if (message[i] != head[i]) {
                return "message is not a CSM/1 write proof pre-image: wrong label"
            }
        }
        val sep = message.size - 32 - 1
        if (message[sep] != 0.toByte()) {
            return "message is not a CSM/1 write proof pre-image: no separator before the digest"
        }
        val mid = message.copyOfRange(head.size, sep)
        val nul = mid.indexOfFirst { it == 0.toByte() }
        if (nul < 0 || mid.drop(nul + 1).any { it == 0.toByte() }) {
            return "message is not a CSM/1 write proof pre-image: wrong number of separators"
        }
        val method = String(mid, 0, nul, Charsets.US_ASCII)
        val path = String(mid, nul + 1, mid.size - nul - 1, Charsets.US_ASCII)
        if (method != "PUT" && method != "POST") {
            return "message is not a CSM/1 write proof pre-image: method $method"
        }
        if (path !in CANONICAL_PATHS) {
            return "message is not a CSM/1 write proof pre-image: path $path"
        }
        // PUT only ever goes to preferences, POST only ever to enrolment.
        if ((method == "PUT") != (path == PATH_PREFERENCES)) {
            return "message is not a CSM/1 write proof pre-image: $method $path"
        }
        return null
    }

    // MARK: identity

    /** Establishes the identity on first use, then returns the cached one. */
    private fun identity(): Identity {
        cached?.let { return it }
        // The generation moves with the key material: an install whose key sat
        // in the seam prefs must keep both, or the agreement point stops
        // matching the generation registered with the operator.
        val generation = prefs.getInt(PREF_GENERATION, legacyPrefs.getInt(PREF_GENERATION, 0))
            .let { if (it == 0) 1 else it }

        val signPair = ensureSigningKey()
        val agreePair = ensureAgreementKey(generation)

        val signTier = tierOf(signPair.first)
        val agreeTier = tierOf(agreePair.first)
        // The weaker half decides: a device is only as protected as its softest
        // key, and rounding up here would be the one lie the operator cannot check.
        val tier = maxOf(signTier, agreeTier)

        val id = Identity(
            signingSpki = spkiOf(signPair.second),
            agreementPoint = pointOf(agreePair.second),
            tier = tier,
            generation = generation,
        )
        prefs.edit().putInt(PREF_GENERATION, generation).apply()
        cached = id
        return id
    }

    /**
     * Returns the signing key pair, creating it on first use.
     *
     * StrongBox first, TEE second, in-process key last. Each step down is a real
     * loss and is reported as one; none of them is silent.
     */
    private fun ensureSigningKey(): Pair<PrivateKey, PublicKey> {
        keystoreEntry(SIGN_ALIAS)?.let { return it }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            generateKeystoreKey(SIGN_ALIAS, KeyProperties.PURPOSE_SIGN, strongBox = true)?.let {
                return keystoreEntry(SIGN_ALIAS) ?: softwareSigningKey()
            }
            generateKeystoreKey(SIGN_ALIAS, KeyProperties.PURPOSE_SIGN, strongBox = false)?.let {
                return keystoreEntry(SIGN_ALIAS) ?: softwareSigningKey()
            }
        }
        return softwareSigningKey()
    }

    /**
     * Returns the agreement key pair of [generation], creating it on first use.
     *
     * `PURPOSE_AGREE_KEY` exists from API 31. Below that the keystore cannot hold
     * an ECDH key at all, so the agreement half is a software key and the tier
     * drops to 3 for the whole device, which is the truth.
     */
    private fun ensureAgreementKey(generation: Int): Pair<PrivateKey, PublicKey> {
        val alias = AGREE_ALIAS_PREFIX + generation
        keystoreEntry(alias)?.let { return it }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            generateKeystoreKey(alias, KeyProperties.PURPOSE_AGREE_KEY, strongBox = true)?.let {
                return keystoreEntry(alias) ?: softwareAgreementKey()
            }
            generateKeystoreKey(alias, KeyProperties.PURPOSE_AGREE_KEY, strongBox = false)?.let {
                return keystoreEntry(alias) ?: softwareAgreementKey()
            }
        }
        return softwareAgreementKey()
    }

    private fun generateKeystoreKey(alias: String, purpose: Int, strongBox: Boolean): KeyPair? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return null
        return try {
            val builder = KeyGenParameterSpec.Builder(alias, purpose)
                .setAlgorithmParameterSpec(ECGenParameterSpec(CURVE))
                .setDigests(KeyProperties.DIGEST_SHA256)
            if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                builder.setIsStrongBoxBacked(true)
            } else if (strongBox) {
                return null
            }
            val gen = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, KEYSTORE)
            gen.initialize(builder.build())
            gen.generateKeyPair()
        } catch (_: Throwable) {
            // StrongBoxUnavailableException is API 28 and cannot be named in a
            // catch clause on older devices without risking a VerifyError, so
            // every refusal lands here.
            // A keystore that refuses the spec (no EC, no ECDH purpose, a locked
            // device policy) is a step down, not a crash: the caller falls through
            // to the software tier and says so.
            null
        }
    }

    private fun keystoreEntry(alias: String): Pair<PrivateKey, PublicKey>? = try {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        val priv = ks.getKey(alias, null) as? PrivateKey
        val cert = ks.getCertificate(alias)
        if (priv != null && cert != null) priv to cert.publicKey else null
    } catch (_: Throwable) {
        null
    }

    private fun softwareSigningKey(): Pair<PrivateKey, PublicKey> =
        softwareKey(PREF_SW_SIGN)

    private fun softwareAgreementKey(): Pair<PrivateKey, PublicKey> =
        softwareKey(PREF_SW_AGREE)

    /**
     * An in-process P-256 key persisted in the app's private prefs. This is the
     * software tier, named as such: the key is readable by anything that can read
     * the app's data directory, and the reported tier is 3.
     */
    private fun softwareKey(prefKey: String): Pair<PrivateKey, PublicKey> {
        val stored = prefs.getString(prefKey, null) ?: migrateLegacySoftwareKey(prefKey)
        val factory = KeyFactory.getInstance("EC")
        if (stored != null) {
            try {
                val priv = factory.generatePrivate(PKCS8EncodedKeySpec(Base64.decode(stored, Base64.NO_WRAP)))
                val pub = prefs.getString(prefKey + ".pub", null)
                if (pub != null) {
                    return priv to factory.generatePublic(X509EncodedKeySpec(Base64.decode(pub, Base64.NO_WRAP)))
                }
            } catch (_: Throwable) {
                // Corrupt material is replaced, not patched around.
            }
        }
        val gen = KeyPairGenerator.getInstance("EC")
        gen.initialize(ECGenParameterSpec(CURVE))
        val pair = gen.generateKeyPair()
        prefs.edit()
            .putString(prefKey, Base64.encodeToString(pair.private.encoded, Base64.NO_WRAP))
            .putString(prefKey + ".pub", Base64.encodeToString(pair.public.encoded, Base64.NO_WRAP))
            .apply()
        return pair.private to pair.public
    }

    /**
     * Moves software key material out of the shared seam prefs into the device
     * prefs file, once, and removes the old copy.
     *
     * An install that already has a key must keep it: regenerating would change
     * the `dtp` and leave a second, unreachable device row at the operator.
     * Returns the material so the caller can use it in the same pass.
     */
    private fun migrateLegacySoftwareKey(prefKey: String): String? {
        val old = legacyPrefs.getString(prefKey, null) ?: return null
        val oldPub = legacyPrefs.getString(prefKey + ".pub", null)
        prefs.edit()
            .putString(prefKey, old)
            .apply { if (oldPub != null) putString(prefKey + ".pub", oldPub) }
            .commit()
        legacyPrefs.edit()
            .remove(prefKey)
            .remove(prefKey + ".pub")
            .apply()
        return old
    }

    private fun signingPrivateKey(): PrivateKey =
        keystoreEntry(SIGN_ALIAS)?.first ?: softwareSigningKey().first

    private fun agreementPrivateKey(generation: Int): PrivateKey? {
        keystoreEntry(AGREE_ALIAS_PREFIX + generation)?.let { return it.first }
        // Only the current generation has software material; an older one that
        // was never created here is genuinely absent, and saying so is what lets
        // the core re-key instead of failing forever (02-SPEC.md 10.3).
        val current = prefs.getInt(PREF_GENERATION, 1)
        return if (generation == current) softwareAgreementKey().first else null
    }

    private fun agreementPoint(generation: Int): ByteArray? {
        keystoreEntry(AGREE_ALIAS_PREFIX + generation)?.let { return pointOf(it.second) }
        val current = prefs.getInt(PREF_GENERATION, 1)
        return if (generation == current) pointOf(softwareAgreementKey().second) else null
    }

    // MARK: tier

    /**
     * The honest tier of one key. Android has no Secure Enclave, so hardware here
     * is always tier 2; the distinction between StrongBox and the TEE is real but
     * the wire vocabulary of 03-WIRE.md 13.8 does not carry it.
     */
    private fun tierOf(priv: PrivateKey): Int = try {
        val factory = KeyFactory.getInstance(priv.algorithm, KEYSTORE)
        val info = factory.getKeySpec(priv, KeyInfo::class.java)
        val hardware = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            info.securityLevel == KeyProperties.SECURITY_LEVEL_STRONGBOX ||
                info.securityLevel == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT
        } else {
            @Suppress("DEPRECATION")
            info.isInsideSecureHardware
        }
        if (hardware) TIER_STRONGBOX_OR_TEE else TIER_SOFTWARE
    } catch (_: Throwable) {
        // Not a keystore key at all: that is the software tier, and it says so.
        TIER_SOFTWARE
    }

    // MARK: encodings

    /** The 65-byte uncompressed point of an EC public key. */
    private fun pointOf(pub: PublicKey): ByteArray {
        val encoded = pub.encoded // X.509 SubjectPublicKeyInfo
        val at = indexOfPrefix(encoded)
        return if (at >= 0) encoded.copyOfRange(at, at + 65) else ByteArray(0)
    }

    /** The DER SubjectPublicKeyInfo of an EC public key, as the wire wants it. */
    private fun spkiOf(pub: PublicKey): ByteArray {
        val point = pointOf(pub)
        return if (point.size == 65) SPKI_PREFIX + point else pub.encoded
    }

    private fun indexOfPrefix(encoded: ByteArray): Int {
        if (encoded.size < SPKI_PREFIX.size + 65) return -1
        for (i in 0..(encoded.size - SPKI_PREFIX.size - 65)) {
            var hit = true
            for (j in SPKI_PREFIX.indices) {
                if (encoded[i + j] != SPKI_PREFIX[j]) {
                    hit = false
                    break
                }
            }
            if (hit) return i + SPKI_PREFIX.size
        }
        return -1
    }

    private fun publicKeyFromPoint(point: ByteArray): PublicKey? = try {
        KeyFactory.getInstance("EC").generatePublic(X509EncodedKeySpec(SPKI_PREFIX + point))
    } catch (_: Throwable) {
        null
    }

    /**
     * ASN.1 DER `SEQUENCE { INTEGER r, INTEGER s }` -> 64 bytes `r || s`.
     *
     * Both platforms return DER by default and a Rust or Go verifier reading raw
     * `r || s` would reject every real device, so the conversion happens at the
     * edge, exactly once, together with the low-`s` normalization that also
     * removes the malleability question instead of leaving it open.
     */
    private fun derToRawSignature(der: ByteArray): ByteArray? {
        if (der.size < 8 || der[0] != 0x30.toByte()) return null
        var i = 1
        if (der[i].toInt() and 0x80 != 0) {
            i += (der[i].toInt() and 0x7f) + 1
        } else {
            i += 1
        }
        if (i >= der.size || der[i] != 0x02.toByte()) return null
        i += 1
        val rLen = der[i].toInt() and 0xff
        i += 1
        if (i + rLen > der.size) return null
        val r = BigInteger(1, der.copyOfRange(i, i + rLen))
        i += rLen
        if (i >= der.size || der[i] != 0x02.toByte()) return null
        i += 1
        val sLen = der[i].toInt() and 0xff
        i += 1
        if (i + sLen > der.size) return null
        var s = BigInteger(1, der.copyOfRange(i, i + sLen))
        if (s > HALF_N) {
            s = N.subtract(s)
        }
        val out = ByteArray(64)
        fill(out, 0, r)
        fill(out, 32, s)
        return out
    }

    private fun fill(dst: ByteArray, offset: Int, v: BigInteger) {
        val bytes = v.toByteArray()
        // BigInteger.toByteArray may carry a leading zero sign byte; the fixed
        // 32-byte field has no room for it and no need of it.
        val start = if (bytes.size > 32) bytes.size - 32 else 0
        val len = minOf(32, bytes.size - start)
        System.arraycopy(bytes, start, dst, offset + (32 - len), len)
    }

    /** `dtp = sha256(device_signing_SPKI_DER)[0..16]`, lower-case hex. */
    private fun dtpHex(spki: ByteArray): String {
        val sum = java.security.MessageDigest.getInstance("SHA-256").digest(spki)
        val sb = StringBuilder(32)
        for (i in 0 until 16) {
            sb.append(String.format("%02x", sum[i]))
        }
        return sb.toString()
    }

    private fun b64(b: ByteArray): String = Base64.encodeToString(b, Base64.NO_WRAP)
}

/**
 * The gomobile-facing adapter: the Go core calls these three methods for every
 * device signature and every key agreement, so the core never holds the private
 * key and the hardware tier is real rather than claimed.
 */
internal class CarambaDeviceKeyBridge(
    private val keys: CarambaDeviceKeys,
) : io.caramba.core.mobile.DeviceKeyBridge {
    override fun keygen(reqJSON: String): String = keys.keygen(reqJSON)
    override fun sign(reqJSON: String): String = keys.sign(reqJSON)
    override fun agree(reqJSON: String): String = keys.agree(reqJSON)
}
