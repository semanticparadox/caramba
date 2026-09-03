import CryptoKit
import Foundation
import Security

// CarambaDeviceKeys is the Apple holder of the CSM/1 device identity
// (02-SPEC.md 9.4, 12.2 and 03-WIRE.md 13.6).
//
// The signing key MUST live in the Secure Enclave, and the Enclave is reachable
// only through SecKeyCreateRandomKey with kSecAttrTokenIDSecureEnclave. Neither
// Go nor Dart can get there: a Go implementation would by definition put the key
// in a file, which is the software tier. So the key lives here, and the Go core
// calls back into this class for every signature and every key agreement.
//
// The tier is reported HONESTLY: 1 when both keys are in the Enclave, 3 when the
// build has no Enclave (the simulator, an old Mac) and the keys are ordinary
// keychain keys. It never claims hardware it does not have.
//
// Two keys, not one, because 03-WIRE.md 13.8 registers both at once and they have
// different purposes: signing the write proof and unsealing the 0x06 directive.
// The reported tier is the weaker of the two, because a device is only as
// protected as its softest half.
//
// CODE IDENTIFIERS stay 'caramba' (the user-facing brand is 'exarobot').
final class CarambaDeviceKeys {

    private enum Tier {
        static let secureEnclave = 1
        static let strongBoxOrTee = 2
        static let software = 3
    }

    private static let signTag = "com.caramba.csm.device.sign".data(using: .utf8)!
    private static let agreeTagPrefix = "com.caramba.csm.device.agree."

    /// The machine reason for "this device holds no agreement key of that
    /// generation", which is seal step 5 and nothing else.
    private static let codeNoAgreementGeneration = "no_agreement_generation"

    /// The one construction the device signing key ever signs
    /// (03-WIRE.md 13.6). Checked on the way in, see `writeProofRejection`.
    private static let writeProofLabel = "csm1-write"
    private static let pathPreferences = "/api/v2/app/preferences"
    private static let canonicalPaths: Set<String> = [
        pathPreferences,
        "/api/v2/app/csm/enroll/code",
        "/api/v2/app/csm/enroll/device",
    ]

    /// Returns a reason when [message] is not the CSM/1 write-proof pre-image:
    ///
    ///   "csm1-write" || 0x00 || method || 0x00 || canonicalPath || 0x00 ||
    ///   sha256(body)
    ///
    /// Read from the END rather than split on the separator: the last 32 bytes
    /// are a digest and a zero byte inside it is perfectly legal.
    static func writeProofRejection(_ message: Data) -> String? {
        let head = Array(writeProofLabel.utf8) + [UInt8(0)]
        let bytes = [UInt8](message)
        guard bytes.count >= head.count + 3 + 32 else {
            return "message is not a CSM/1 write proof pre-image: too short"
        }
        guard Array(bytes[0..<head.count]) == head else {
            return "message is not a CSM/1 write proof pre-image: wrong label"
        }
        let sep = bytes.count - 32 - 1
        guard bytes[sep] == 0 else {
            return "message is not a CSM/1 write proof pre-image: no separator before the digest"
        }
        let mid = Array(bytes[head.count..<sep])
        guard let nul = mid.firstIndex(of: 0),
              !mid[(nul + 1)...].contains(0)
        else {
            return "message is not a CSM/1 write proof pre-image: wrong number of separators"
        }
        let method = String(decoding: mid[0..<nul], as: UTF8.self)
        let path = String(decoding: mid[(nul + 1)...], as: UTF8.self)
        guard method == "PUT" || method == "POST" else {
            return "message is not a CSM/1 write proof pre-image: method \(method)"
        }
        guard canonicalPaths.contains(path) else {
            return "message is not a CSM/1 write proof pre-image: path \(path)"
        }
        // PUT only ever goes to preferences, POST only ever to enrolment.
        guard (method == "PUT") == (path == pathPreferences) else {
            return "message is not a CSM/1 write proof pre-image: \(method) \(path)"
        }
        return nil
    }

    /// The generation of the agreement key, `rkv`. Starts at 1 and only ever
    /// grows; a re-key writes a new tag rather than replacing the old key,
    /// because a sealed directive addressed to the previous generation must
    /// still open (02-SPEC.md 10.3).
    private static let generationDefaultsKey = "com.caramba.csm.device.generation"

    /// DER SubjectPublicKeyInfo prefix for an uncompressed P-256 public key:
    /// SEQUENCE { SEQUENCE { id-ecPublicKey, prime256v1 }, BIT STRING }.
    private static let spkiPrefix: [UInt8] = [
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02,
        0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03,
        0x42, 0x00,
    ]

    private let lock = NSLock()
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    // MARK: bridge surface (the JSON shapes of ABI v3)

    /// `{"purpose":"sign"|"agree","require_hardware":bool}` ->
    /// `{"spki_b64","agree_pub_b64","dtp_hex","tier","generation"}`.
    ///
    /// Idempotent: the identity is established once and survives restarts. A new
    /// identity per launch would mean a new `dtp`, and therefore a new device in
    /// the operator's list after every start of the app.
    func keygen(_ requestJSON: String) -> String {
        lock.lock()
        defer { lock.unlock() }
        let generation = currentGeneration()
        guard let signKey = ensureKey(tag: Self.signTag, canSign: true),
              let agreeKey = ensureKey(tag: agreeTag(generation), canSign: false)
        else {
            return errorJSON("the device key store refused to create a P-256 key")
        }
        guard let signPub = publicPoint(of: signKey), let agreePub = publicPoint(of: agreeKey) else {
            return errorJSON("the device public key is not an uncompressed P-256 point")
        }
        let spki = Data(Self.spkiPrefix) + signPub
        // The weaker half decides: rounding up here would be the one lie the
        // operator has no way to check.
        let tier = max(tierOf(signKey), tierOf(agreeKey))
        return json([
            "spki_b64": spki.base64EncodedString(),
            "agree_pub_b64": agreePub.base64EncodedString(),
            "dtp_hex": dtpHex(spki),
            "tier": tier,
            "generation": generation,
        ])
    }

    /// `{"message_b64"}` -> `{"sig_b64"}`, 64 bytes `r || s` with low `s`.
    ///
    /// `.ecdsaSignatureMessageX962SHA256` is a MESSAGE algorithm: it hashes the
    /// input itself, which is what 03-WIRE.md 13.6 fixes. A signer that
    /// pre-hashed and then signed the digest as a message would build the right
    /// pre-image and still produce a signature the panel rejects.
    func sign(_ requestJSON: String) -> String {
        lock.lock()
        defer { lock.unlock() }
        guard let obj = object(from: requestJSON),
              let b64 = obj["message_b64"] as? String,
              let message = Data(base64Encoded: b64), !message.isEmpty
        else {
            return errorJSON("empty message is not signed")
        }
        // The one construction this key ever signs (03-WIRE.md 13.6), checked
        // rather than assumed. The Secure Enclave makes the key
        // non-extractable, and a holder that signs arbitrary bytes stands in
        // for extraction: anything running in this process (a third-party
        // Flutter package, a second isolate, injected code) would otherwise get
        // a signature over an enrolment body aimed at a hostile origin, over a
        // rekey proof, or over a preferences write on another canonical path.
        if let reason = Self.writeProofRejection(message) {
            return errorJSON(reason)
        }
        guard let key = ensureKey(tag: Self.signTag, canSign: true) else {
            return errorJSON("the device signing key is unavailable")
        }
        var error: Unmanaged<CFError>?
        guard let der = SecKeyCreateSignature(
            key, .ecdsaSignatureMessageX962SHA256, message as CFData, &error
        ) as Data? else {
            return errorJSON(describe(error) ?? "the device signature failed")
        }
        guard let raw = Self.rawSignature(fromDER: der) else {
            return errorJSON("the platform returned a signature that is not ASN.1 ECDSA")
        }
        return json(["sig_b64": raw.base64EncodedString()])
    }

    /// `{"rkv","peer_pub_b64"}` -> `{"shared_b64","own_pub_b64"}`.
    ///
    /// `own_pub_b64` travels with the secret because DHKEM's `kem_context`
    /// carries the recipient's own public key, and only the key holder knows it.
    func agree(_ requestJSON: String) -> String {
        lock.lock()
        defer { lock.unlock() }
        guard let obj = object(from: requestJSON),
              let b64 = obj["peer_pub_b64"] as? String,
              let peerPoint = Data(base64Encoded: b64),
              peerPoint.count == 65, peerPoint.first == 0x04
        else {
            return errorJSON("peer key is not an uncompressed P-256 point")
        }
        let asked = (obj["rkv"] as? NSNumber)?.intValue ?? 0
        let generation = asked == 0 ? currentGeneration() : asked
        // An absent generation is said plainly rather than answered with a wrong
        // secret: that is what lets the core re-key instead of failing forever.
        guard let key = existingKey(tag: agreeTag(generation)) else {
            // The MACHINE reason, not just the prose. Seal step 5 tells the
            // client to rekey and re-request (02-SPEC.md 10.3); step 6 tells it
            // nothing of the kind, and a bridge that answers both with free
            // text makes the core burn a key generation on every corrupted enc.
            return json([
                "error": "this device holds no agreement key of generation \(generation)",
                "code": Self.codeNoAgreementGeneration,
            ])
        }
        var error: Unmanaged<CFError>?
        guard let peer = SecKeyCreateWithData(
            peerPoint as CFData,
            [
                kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom,
                kSecAttrKeyClass: kSecAttrKeyClassPublic,
                kSecAttrKeySizeInBits: 256,
            ] as CFDictionary,
            &error
        ) else {
            return errorJSON(describe(error) ?? "peer key does not decode")
        }
        guard let shared = SecKeyCopyKeyExchangeResult(
            key, .ecdhKeyExchangeStandard, peer, [:] as CFDictionary, &error
        ) as Data? else {
            return errorJSON(describe(error) ?? "key agreement failed")
        }
        guard let own = publicPoint(of: key) else {
            return errorJSON("the agreement public key is missing")
        }
        return json([
            "shared_b64": shared.base64EncodedString(),
            "own_pub_b64": own.base64EncodedString(),
        ])
    }

    // MARK: keys

    private func agreeTag(_ generation: Int) -> Data {
        (Self.agreeTagPrefix + String(generation)).data(using: .utf8)!
    }

    private func currentGeneration() -> Int {
        let stored = defaults.integer(forKey: Self.generationDefaultsKey)
        if stored > 0 { return stored }
        defaults.set(1, forKey: Self.generationDefaultsKey)
        return 1
    }

    /// Returns the key for [tag], creating it in the Secure Enclave when the
    /// build has one and falling back to an ordinary keychain key when it does
    /// not. The fallback is a real loss and the tier says so.
    private func ensureKey(tag: Data, canSign: Bool) -> SecKey? {
        if let existing = existingKey(tag: tag) { return existing }
        if let enclave = createKey(tag: tag, inSecureEnclave: true, canSign: canSign) {
            return enclave
        }
        return createKey(tag: tag, inSecureEnclave: false, canSign: canSign)
    }

    private func existingKey(tag: Data) -> SecKey? {
        // kSecAttrKeyClass pins the match to the PRIVATE half, and
        // kSecUseDataProtectionKeychain puts the item in the data-protection
        // keychain on macOS as well as iOS. Without the latter, the non-Enclave
        // fallback created by createKey(inSecureEnclave: false) lands in the
        // file-based login keychain, where the
        // kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly it asks for is not
        // honoured at all: the item becomes exportable and rides along in
        // keychain backups, which is not what tier 3 is supposed to mean.
        let query: [CFString: Any] = [
            kSecClass: kSecClassKey,
            kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeyClass: kSecAttrKeyClassPrivate,
            kSecAttrApplicationTag: tag,
            kSecUseDataProtectionKeychain: true,
            kSecReturnRef: true,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess else { return nil }
        // A CFTypeRef that is not a SecKey is a corrupt keychain entry, not a key.
        guard let ref = item, CFGetTypeID(ref) == SecKeyGetTypeID() else { return nil }
        return (ref as! SecKey)
    }

    private func createKey(tag: Data, inSecureEnclave: Bool, canSign: Bool) -> SecKey? {
        var privateAttrs: [CFString: Any] = [
            kSecAttrIsPermanent: true,
            kSecAttrApplicationTag: tag,
        ]
        if inSecureEnclave {
            // .privateKeyUsage and nothing else: no biometry and no passcode
            // prompt. A settings write and a directive unseal happen while the
            // app is in the background, and a key that needs a face there is a
            // key that silently stops working.
            guard let access = SecAccessControlCreateWithFlags(
                kCFAllocatorDefault,
                kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
                .privateKeyUsage,
                nil
            ) else { return nil }
            privateAttrs[kSecAttrAccessControl] = access
        } else {
            privateAttrs[kSecAttrAccessible] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        }
        var attrs: [CFString: Any] = [
            kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits: 256,
            kSecPrivateKeyAttrs: privateAttrs,
            // Same reason as in existingKey: on macOS the file-based login
            // keychain ignores kSecAttrAccessible and keeps the item
            // exportable and backed up.
            kSecUseDataProtectionKeychain: true,
        ]
        if inSecureEnclave {
            attrs[kSecAttrTokenID] = kSecAttrTokenIDSecureEnclave
        }
        _ = canSign // both algorithms are available on one P-256 key; the tags differ, not the capability
        var error: Unmanaged<CFError>?
        let key = SecKeyCreateRandomKey(attrs as CFDictionary, &error)
        // A refusal is a step down to the next tier, not a crash: the caller
        // falls through to the keychain key and the reported tier says 3.
        _ = error?.takeRetainedValue()
        return key
    }

    /// The honest tier of one key: 1 when it sits in the Secure Enclave, 3 when
    /// it is an ordinary keychain key.
    private func tierOf(_ key: SecKey) -> Int {
        guard let attrs = SecKeyCopyAttributes(key) as? [CFString: Any] else {
            return Tier.software
        }
        if let token = attrs[kSecAttrTokenID] as? String,
           token == (kSecAttrTokenIDSecureEnclave as String) {
            return Tier.secureEnclave
        }
        return Tier.software
    }

    /// The 65-byte uncompressed point of the key's public half.
    private func publicPoint(of key: SecKey) -> Data? {
        guard let pub = SecKeyCopyPublicKey(key) else { return nil }
        var error: Unmanaged<CFError>?
        guard let raw = SecKeyCopyExternalRepresentation(pub, &error) as Data? else {
            _ = error?.takeRetainedValue()
            return nil
        }
        return raw.count == 65 && raw.first == 0x04 ? raw : nil
    }

    // MARK: encodings

    /// ASN.1 DER `SEQUENCE { INTEGER r, INTEGER s }` -> 64 bytes `r || s`.
    ///
    /// Both platforms return DER by default and a Rust or Go verifier reading raw
    /// `r || s` would reject every real device, so the conversion happens at the
    /// edge, exactly once, together with the low-`s` normalization that also
    /// removes the malleability question instead of leaving it open.
    static func rawSignature(fromDER der: Data) -> Data? {
        let b = [UInt8](der)
        guard b.count >= 8, b[0] == 0x30 else { return nil }
        var i = 1
        if b[i] & 0x80 != 0 {
            i += Int(b[i] & 0x7f) + 1
        } else {
            i += 1
        }
        guard i < b.count, b[i] == 0x02 else { return nil }
        i += 1
        guard i < b.count else { return nil }
        let rLen = Int(b[i]); i += 1
        guard i + rLen <= b.count else { return nil }
        let r = Array(b[i..<(i + rLen)]); i += rLen
        guard i < b.count, b[i] == 0x02 else { return nil }
        i += 1
        guard i < b.count else { return nil }
        let sLen = Int(b[i]); i += 1
        guard i + sLen <= b.count else { return nil }
        let s = Array(b[i..<(i + sLen)])

        var out = [UInt8](repeating: 0, count: 64)
        placeBigEndian(r, into: &out, at: 0)
        placeBigEndian(lowS(s), into: &out, at: 32)
        return Data(out)
    }

    /// The order of the P-256 group, big-endian.
    private static let curveOrder: [UInt8] = [
        0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84,
        0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
    ]

    /// `n - s` when `s > n/2`, otherwise `s`. Implemented on bytes rather than
    /// through a big-integer dependency: it is one comparison and one subtraction
    /// on a fixed 32-byte field, and adding a dependency for that would be worse.
    private static func lowS(_ s: [UInt8]) -> [UInt8] {
        var value = [UInt8](repeating: 0, count: 32)
        placeBigEndian(s, into: &value, at: 0)
        // s > n/2 iff 2*s > n. Comparing 2*s against n avoids halving n.
        var doubled = [UInt8](repeating: 0, count: 33)
        var carry: UInt16 = 0
        for i in stride(from: 31, through: 0, by: -1) {
            let sum = UInt16(value[i]) << 1 | carry
            doubled[i + 1] = UInt8(sum & 0xff)
            carry = sum >> 8
        }
        doubled[0] = UInt8(carry)
        var greater = false
        if doubled[0] != 0 {
            greater = true
        } else {
            for i in 0..<32 {
                if doubled[i + 1] != curveOrder[i] {
                    greater = doubled[i + 1] > curveOrder[i]
                    break
                }
            }
        }
        if !greater { return value }
        var out = [UInt8](repeating: 0, count: 32)
        var borrow: Int = 0
        for i in stride(from: 31, through: 0, by: -1) {
            let diff = Int(curveOrder[i]) - Int(value[i]) - borrow
            if diff < 0 {
                out[i] = UInt8(diff + 256)
                borrow = 1
            } else {
                out[i] = UInt8(diff)
                borrow = 0
            }
        }
        return out
    }

    /// Right-aligns a big-endian integer into a 32-byte slot, dropping the
    /// leading sign byte DER adds and refusing to lose significant bytes.
    private static func placeBigEndian(_ value: [UInt8], into dst: inout [UInt8], at offset: Int) {
        var v = value
        while v.count > 32, v.first == 0x00 { v.removeFirst() }
        if v.count > 32 { v = Array(v.suffix(32)) }
        let start = offset + (32 - v.count)
        for (k, byte) in v.enumerated() {
            dst[start + k] = byte
        }
    }

    /// `dtp = sha256(device_signing_SPKI_DER)[0..16]`, lower-case hex.
    private func dtpHex(_ spki: Data) -> String {
        let digest = SHA256.hash(data: spki)
        return digest.prefix(16).map { String(format: "%02x", $0) }.joined()
    }

    // MARK: JSON

    /// Builds a request body for the three bridge methods. Serialization rather
    /// than interpolation: base64 happens to be JSON-safe, but a request shape
    /// that only works because of what its values happen to contain is a bug
    /// waiting for the first value that does not.
    static func request(_ payload: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              let text = String(data: data, encoding: .utf8)
        else { return "{}" }
        return text
    }

    private func object(from source: String) -> [String: Any]? {
        guard let data = source.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        return obj
    }

    private func json(_ payload: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              let text = String(data: data, encoding: .utf8)
        else { return #"{"error":"the device key bridge could not encode its reply"}"# }
        return text
    }

    private func errorJSON(_ message: String) -> String {
        json(["error": message])
    }

    private func describe(_ error: Unmanaged<CFError>?) -> String? {
        guard let error = error else { return nil }
        let value = error.takeRetainedValue()
        return CFErrorCopyDescription(value) as String?
    }
}

#if canImport(Caramba)
import Caramba

/// The gomobile-facing adapter: the Go core calls these three methods for every
/// device signature and every key agreement, so the core never holds the private
/// key and the Secure Enclave tier is real rather than claimed.
///
/// gomobile turns `mobile.DeviceKeyBridge` into the `CarambaDeviceKeyBridge`
/// protocol, whose methods return `(String, Error)`; a Swift implementation
/// returns the string and writes no error, because every refusal is already a
/// JSON `{"error":...}` the core understands.
final class CarambaGoDeviceKeyBridge: NSObject, CarambaDeviceKeyBridge {
    private let keys: CarambaDeviceKeys

    init(keys: CarambaDeviceKeys) {
        self.keys = keys
        super.init()
    }

    func keygen(_ reqJSON: String?) throws -> String {
        keys.keygen(reqJSON ?? "{}")
    }

    func sign(_ reqJSON: String?) throws -> String {
        keys.sign(reqJSON ?? "{}")
    }

    func agree(_ reqJSON: String?) throws -> String {
        keys.agree(reqJSON ?? "{}")
    }
}
#endif
