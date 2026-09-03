//! Маршруты CSM/1 (`03-WIRE.md` раздел 13).
//!
//! Отдача документа это ЧТЕНИЕ, а не подписание. Причина в `03-WIRE.md` 1.5:
//! `iat` и `exp` входят в подписываемый payload, поэтому пересборка в другую
//! секунду даёт другой кадр и другой хэш, а хэш каталога опубликован в
//! ключевом документе. Подписывать на каждый запрос значило бы обесценивать
//! эту привязку при каждом перезапуске панели.
//!
//! Ключевой документ здесь исключение и подписывается на лету только потому,
//! что его содержимое (набор ключей, пороги, отзывы) меняется редко и целиком
//! лежит в базе. Как только появится хранилище подписанных ключевых документов,
//! этот путь станет таким же чтением.

use axum::{
    Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use caramba_shared::csm::{self, DocType};

use crate::AppState;

/// Тип содержимого подписанных документов протокола.
const CSM_CONTENT_TYPE: &str = "application/vnd.caramba.csm";

/// Маршруты протокола. Монтируются в корень рядом с `/sub/{uuid}`, потому что
/// периметр `caramba-sub` проксирует именно `/sub/*` и `/api/*`.
pub fn routes() -> Router<AppState> {
    Router::new().route("/sub/k1", get(key_document))
}

/// Ответ с подписанным кадром.
fn frame_response(frame: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(CSM_CONTENT_TYPE),
            ),
            // Документ подписан, поэтому его можно кэшировать где угодно:
            // подделка не пройдёт проверку, а устаревание ловит exp и версия.
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            ),
        ],
        frame,
    )
        .into_response()
}

/// Тело ошибки пустое намеренно (`03-WIRE.md` 13): причина отказа, которую
/// клиент вправе показать пользователю, приходит подписанной внутри директивы,
/// а не текстом, которому нечем доверять.
fn refuse(status: StatusCode) -> Response {
    status.into_response()
}

/// `GET /sub/k1` — ключевой документ, якорь доверия тенанта.
///
/// Панель НЕ подписывает его: корневой приватный ключ живёт офлайн у оператора,
/// и в этом весь смысл разделения ролей. Взлом работающей панели даёт
/// онлайн-ключ, но не даёт подменить якорь. Поэтому маршрут отдаёт то, что
/// оператор подписал у себя и импортировал, а если импорта не было, честно
/// отвечает 503 вместо документа, подписанного не той ролью.
async fn key_document(State(state): State<AppState>) -> Response {
    serve_root_document(&state, DocType::Key).await
}

/// Отдаёт последнюю версию корневого документа заданного типа.
async fn serve_root_document(state: &AppState, doc_type: DocType) -> Response {
    match latest_root_document(&state.pool, doc_type).await {
        Ok(Some(frame)) => frame_response(frame),
        Ok(None) => refuse(StatusCode::SERVICE_UNAVAILABLE),
        Err(e) => {
            tracing::error!(error = %e, doc_type = doc_type.as_u8(), "csm: чтение корневого документа");
            refuse(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Последняя импортированная версия документа.
async fn latest_root_document(
    pool: &sqlx::PgPool,
    doc_type: DocType,
) -> anyhow::Result<Option<Vec<u8>>> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT frame FROM csm_root_documents WHERE doc_type = $1 ORDER BY ver DESC LIMIT 1",
    )
    .bind(doc_type.as_u8() as i16)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(frame,)| frame))
}

/// Импортирует документ, подписанный офлайн корневым ключом.
///
/// Проверяет только то, что панель обязана проверить, чтобы не отдавать заведомо
/// нерабочий кадр: магию, тип, правило точной длины и совпадение `pid` с
/// личностью тенанта. Подпись проверяет клиент; панель не является доверенной
/// стороной для собственного якоря.
pub fn validate_root_frame(
    frame: &[u8],
    expected: DocType,
    identity_pid: &[u8; 8],
) -> anyhow::Result<()> {
    use anyhow::{anyhow, ensure};

    ensure!(frame.len() > 8, "кадр короче минимального");
    ensure!(frame[..4] == csm::MAGIC, "не кадр CSM/1: магия не совпала");
    ensure!(
        frame[4] == expected.as_u8(),
        "тип документа {} вместо ожидаемого {}",
        frame[4],
        expected.as_u8()
    );

    let payload_len = u16::from_be_bytes([frame[5], frame[6]]) as usize;
    let nsigs_off = 7 + payload_len;
    ensure!(frame.len() > nsigs_off, "длина payload выходит за кадр");
    let nsigs = frame[nsigs_off] as usize;
    ensure!((1..=4).contains(&nsigs), "число подписей вне диапазона");
    let expected_len = 7 + payload_len + 1 + 76 * nsigs;
    ensure!(
        frame.len() == expected_len,
        "правило точной длины нарушено: {} вместо {}",
        frame.len(),
        expected_len
    );

    // pid лежит в конверте под ключом 2 как bstr(8): `02 48 <8 байт>`.
    let payload = &frame[7..7 + payload_len];
    let pid_at = payload
        .windows(2)
        .position(|w| w == [0x02, 0x48])
        .ok_or_else(|| anyhow!("в конверте нет поля pid"))?;
    let pid = &payload[pid_at + 2..pid_at + 10];
    ensure!(pid == identity_pid, "документ подписан для другого тенанта");

    Ok(())
}

/// Сохраняет проверенный корневой документ. Возвращает hex sha256 кадра.
///
/// Это вход офлайн-церемонии: оператор подписал документ у себя, панель его
/// принимает, проверяет форму и отдаёт клиентам как есть.
pub async fn import_root_document(
    pool: &sqlx::PgPool,
    doc_type: DocType,
    frame: &[u8],
    identity_pid: &[u8; 8],
) -> anyhow::Result<String> {
    validate_root_frame(frame, doc_type, identity_pid)?;
    let ver = envelope_version(frame)?;
    let hash = super::hex(&csm::frame_digest(frame));

    sqlx::query(
        "INSERT INTO csm_root_documents (doc_type, ver, frame, frame_hash) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (doc_type, ver) DO UPDATE \
           SET frame = EXCLUDED.frame, frame_hash = EXCLUDED.frame_hash, imported_at = NOW()",
    )
    .bind(doc_type.as_u8() as i16)
    .bind(ver as i64)
    .bind(frame)
    .bind(&hash)
    .execute(pool)
    .await?;

    Ok(hash)
}

/// Достаёт `ver` из конверта: ключ 3, беззнаковое целое кратчайшей формы.
fn envelope_version(frame: &[u8]) -> anyhow::Result<u64> {
    use anyhow::{anyhow, ensure};
    let payload_len = u16::from_be_bytes([frame[5], frame[6]]) as usize;
    let payload = &frame[7..7 + payload_len];
    // Конверт фиксирован: a7.. 01 01 02 48 <8> 03 <ver>. Ищем ключ 3 сразу за pid.
    let pid_at = payload
        .windows(2)
        .position(|w| w == [0x02, 0x48])
        .ok_or_else(|| anyhow!("в конверте нет поля pid"))?;
    let at = pid_at + 10;
    ensure!(
        payload.len() > at + 1 && payload[at] == 0x03,
        "в конверте нет поля ver"
    );
    let head = payload[at + 1];
    Ok(match head {
        0x00..=0x17 => head as u64,
        0x18 => payload[at + 2] as u64,
        0x19 => u16::from_be_bytes([payload[at + 2], payload[at + 3]]) as u64,
        0x1a => u32::from_be_bytes([
            payload[at + 2],
            payload[at + 3],
            payload[at + 4],
            payload[at + 5],
        ]) as u64,
        other => return Err(anyhow!("неподдерживаемая голова ver: {other:#04x}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn corpus(name: &str) -> Option<Vec<u8>> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/caramba-client/docs/protocol/05-TEST-VECTORS")
            .join(name);
        std::fs::read(p).ok()
    }

    /// pid тенанта корпуса.
    const CORPUS_PID: [u8; 8] = [0x22, 0x6e, 0x8a, 0x20, 0xf6, 0x99, 0xb9, 0x64];

    #[test]
    fn accepts_the_reference_key_document() {
        let Some(frame) = corpus("bin/positive/k1_min.bin") else {
            return;
        };
        validate_root_frame(&frame, DocType::Key, &CORPUS_PID).expect("эталонный кадр принят");
        assert_eq!(envelope_version(&frame).unwrap(), 1);
    }

    #[test]
    fn rejects_a_trailing_byte() {
        let Some(mut frame) = corpus("bin/positive/k1_min.bin") else {
            return;
        };
        frame.push(0x00);
        let err = validate_root_frame(&frame, DocType::Key, &CORPUS_PID).unwrap_err();
        assert!(err.to_string().contains("точной длины"), "{err}");
    }

    #[test]
    fn rejects_the_wrong_document_type() {
        let Some(frame) = corpus("bin/positive/k1_min.bin") else {
            return;
        };
        assert!(validate_root_frame(&frame, DocType::Bootstrap, &CORPUS_PID).is_err());
    }

    #[test]
    fn rejects_another_tenant() {
        let Some(frame) = corpus("bin/positive/k1_min.bin") else {
            return;
        };
        let err = validate_root_frame(&frame, DocType::Key, &[0u8; 8]).unwrap_err();
        assert!(err.to_string().contains("другого тенанта"), "{err}");
    }

    #[test]
    fn accepts_the_reference_bootstrap_blob() {
        let Some(frame) = corpus("bin/positive/b1_wire_8_5.bin") else {
            return;
        };
        validate_root_frame(&frame, DocType::Bootstrap, &CORPUS_PID).expect("блоб принят");
    }
}
