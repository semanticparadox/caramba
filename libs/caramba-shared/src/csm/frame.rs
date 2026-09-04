//! Кадр CSM/1 и подпись (`03-WIRE.md` разделы 1 и 2).
//!
//! Кадр это плоская строка байт без вложенности:
//! `magic(4) || doc_type(1) || payload_len(2 BE) || payload || nsigs(1) ||
//! nsigs * (keyid_trunc(12) || sig(64))`.
//!
//! Подписывается ровно префикс `7 + payload_len` байт, как он передаётся.
//! Ни подписчик, ни верификатор не пересобирают payload: magic и doc_type
//! внутри подписываемого куска, поэтому подпись каталога нельзя выдать за
//! подпись директивы.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

/// Магия кадра, `"CSM1"`.
pub const MAGIC: [u8; 4] = *b"CSM1";

/// Максимальная длина payload (`03-WIRE.md` 1).
pub const MAX_PAYLOAD_LEN: usize = 49152;

/// Максимум слотов подписи. Потолок существует, чтобы враждебный кадр не
/// заставил верификатор выполнить 255 проверок Ed25519.
pub const MAX_SIGS: usize = 4;

/// Тип документа (`03-WIRE.md` 1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DocType {
    Key = 0x01,
    Catalog = 0x02,
    Directive = 0x03,
    CatalogChunk = 0x04,
    Bootstrap = 0x05,
    SealedDirective = 0x06,
    ReservePool = 0x08,
}

impl DocType {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Ошибка сборки кадра.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// Payload пуст или длиннее предела.
    PayloadLen(usize),
    /// Ноль подписей или больше [`MAX_SIGS`].
    SigCount(usize),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::PayloadLen(n) => write!(
                f,
                "csm frame: длина payload {n} вне диапазона 1..={MAX_PAYLOAD_LEN}"
            ),
            FrameError::SigCount(n) => {
                write!(
                    f,
                    "csm frame: число подписей {n} вне диапазона 1..={MAX_SIGS}"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// Идентификатор ключа: первые 12 байт sha256 от публичного ключа. Это
/// подсказка для поиска, а не право подписи: право даёт таблица ролей.
pub fn keyid_trunc(public_key: &[u8; 32]) -> [u8; 12] {
    let digest = Sha256::digest(public_key);
    let mut out = [0u8; 12];
    out.copy_from_slice(&digest[..12]);
    out
}

/// Подписываемый префикс кадра: `magic || doc_type || u16be(len) || payload`.
pub fn pre_image(doc_type: DocType, payload: &[u8]) -> Vec<u8> {
    let mut pre = Vec::with_capacity(7 + payload.len());
    pre.extend_from_slice(&MAGIC);
    pre.push(doc_type.as_u8());
    pre.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pre.extend_from_slice(payload);
    pre
}

/// Собирает кадр и подписывает его переданными ключами.
///
/// Слоты сортируются по возрастанию `keyid_trunc`: это делает кадр с данным
/// набором подписантов побайтово единственным, а значит каталог можно
/// адресовать по его хэшу. Подпись детерминированная (чистый Ed25519 из
/// RFC 8032), повторное подписание тех же байт даёт тот же кадр.
pub fn build(
    doc_type: DocType,
    payload: &[u8],
    signers: &[SigningKey],
) -> Result<Vec<u8>, FrameError> {
    if payload.is_empty() || payload.len() > MAX_PAYLOAD_LEN {
        return Err(FrameError::PayloadLen(payload.len()));
    }
    if signers.is_empty() || signers.len() > MAX_SIGS {
        return Err(FrameError::SigCount(signers.len()));
    }

    let pre = pre_image(doc_type, payload);

    let mut slots: Vec<([u8; 12], [u8; 64])> = signers
        .iter()
        .map(|sk| {
            let kid = keyid_trunc(&sk.verifying_key().to_bytes());
            let sig = sk.sign(&pre).to_bytes();
            (kid, sig)
        })
        .collect();
    slots.sort_by_key(|slot| slot.0);

    // Одинаковый keyid в двух слотах верификатор отвергает, поэтому не выпускаем.
    if slots.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err(FrameError::SigCount(slots.len()));
    }

    let mut frame = pre;
    frame.push(slots.len() as u8);
    for (kid, sig) in &slots {
        frame.extend_from_slice(kid);
        frame.extend_from_slice(sig);
    }
    debug_assert_eq!(frame.len(), 7 + payload.len() + 1 + 76 * slots.len());
    Ok(frame)
}

/// sha256 всего кадра: по нему каталог адресуется в ключевом документе.
pub fn frame_digest(frame: &[u8]) -> [u8; 32] {
    let d = Sha256::digest(frame);
    let mut out = [0u8; 32];
    out.copy_from_slice(&d);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: [u8; 32]) -> SigningKey {
        SigningKey::from_bytes(&seed)
    }

    #[test]
    fn exact_length_rule_holds() {
        let payload = vec![0xa1, 0x01, 0x01];
        let frame = build(DocType::Directive, &payload, &[key([7u8; 32])]).unwrap();
        assert_eq!(frame.len(), 7 + payload.len() + 1 + 76);
        assert_eq!(&frame[..4], &MAGIC);
        assert_eq!(frame[4], 0x03);
        assert_eq!(
            u16::from_be_bytes([frame[5], frame[6]]) as usize,
            payload.len()
        );
        assert_eq!(frame[7 + payload.len()], 1);
    }

    #[test]
    fn signature_covers_magic_and_doc_type() {
        let payload = vec![0xa1, 0x01, 0x01];
        let a = build(DocType::Directive, &payload, &[key([7u8; 32])]).unwrap();
        let b = build(DocType::Catalog, &payload, &[key([7u8; 32])]).unwrap();
        // Один и тот же payload под разными типами даёт разные подписи: подпись
        // каталога нельзя переиграть как подпись директивы.
        assert_ne!(a[a.len() - 64..], b[b.len() - 64..]);
    }

    #[test]
    fn slots_are_sorted_by_keyid() {
        let keys = [key([1u8; 32]), key([2u8; 32]), key([3u8; 32])];
        let frame = build(DocType::Key, &[0xa1, 0x01, 0x01], &keys).unwrap();
        let base = 7 + 3 + 1;
        let mut prev = [0u8; 12];
        for i in 0..3 {
            let off = base + i * 76;
            let kid: [u8; 12] = frame[off..off + 12].try_into().unwrap();
            assert!(kid > prev, "слоты не отсортированы");
            prev = kid;
        }
    }

    #[test]
    fn signing_is_deterministic() {
        let payload = vec![0xa1, 0x01, 0x01];
        let one = build(DocType::Key, &payload, &[key([9u8; 32])]).unwrap();
        let two = build(DocType::Key, &payload, &[key([9u8; 32])]).unwrap();
        assert_eq!(one, two, "подпись обязана быть детерминированной");
    }
}
