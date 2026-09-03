//! CSM/1, Caramba Signed Manifest: сторона подписи.
//!
//! Панель подписывает документы, клиент их проверяет. Поэтому здесь есть
//! кодировщик строгого профиля CBOR, сборка кадра и подпись Ed25519, и нет
//! верификатора: проверяют документы клиентские реализации (Go в
//! `libs/caramba-core/csm`, Dart в `packages/caramba_vpn`), и обе сверены с
//! общим корпусом векторов.
//!
//! Гарантия, что панель и клиенты понимают одни и те же байты, даётся тестом
//! `tests/csm_vectors.rs`: он воспроизводит эталонные документы корпуса из тех
//! же входных данных и сравнивает побайтово. Расхождение это падающая сборка,
//! а не сюрприз в поле.
//!
//! Спецификация: `apps/caramba-client/docs/protocol/03-WIRE.md` (формат) и
//! `02-SPEC.md` (нормативные правила).

pub mod cbor;
pub mod docs;
pub mod frame;

pub use cbor::Value;
pub use docs::{
    ALG_ED25519, BootstrapBlob, DOC_FRAME_MAX, KeyDocument, KeyEntry, LIFETIME_BOOTSTRAP,
    LIFETIME_CATALOG, LIFETIME_DIRECTIVE, LIFETIME_KEY, LIFETIME_RESERVE, Mirror, Role, RoleSpec,
    SPEC_VERSION,
};
pub use frame::{DocType, FrameError, MAGIC, build, frame_digest, keyid_trunc, pre_image};

use sha2::{Digest, Sha256};

/// Идентификатор тенанта: первые 8 байт sha256 от корневого публичного ключа.
pub fn pid_of(root_public_key: &[u8; 32]) -> [u8; 8] {
    let d = Sha256::digest(root_public_key);
    let mut out = [0u8; 8];
    out.copy_from_slice(&d[..8]);
    out
}
