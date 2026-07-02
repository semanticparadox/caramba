// Plisio — глобальный криптоплатёжный процессор, простой API.
// API: https://api.plisio.net/api/v1/
// Аутентификация: query-параметр api_key.
//
// Подпись вебхука (поле `verify_hash` в теле):
//   Plisio по умолчанию отправляет callback как PHP-serialize над form-data
//   (verify_hash = HMAC-SHA1 от `serialize(ksort($_POST без verify_hash))`).
//   Чтобы получать JSON и иметь воспроизводимую подпись из не-PHP кода,
//   в callback_url ОБЯЗАТЕЛЬНО нужен параметр `json=true`. Тогда:
//     verify_hash = hex( HMAC-SHA1( JSON-тело без поля verify_hash, key = api_key ) )
//   где JSON сериализуется компактно (без пробелов) и в ПОРЯДКЕ ВСТАВКИ
//   (НЕ отсортированный). Источники (подтверждено 2026-06):
//     - офиц. Node-пример из доки Plisio: `{...data}; delete verify_hash; JSON.stringify`
//     - офиц. python-SDK `validate_callback` (insertion order, separators=(',',':'))
//     - WooCommerce-плагин использует PHP-serialize ветку (default, без json=true)
//   Так как в проекте serde_json собран БЕЗ feature `preserve_order`,
//   `serde_json::Value`-объект сортирует ключи (BTreeMap) — повторная сериализация
//   ломала бы порядок. Поэтому поле `verify_hash` вырезается из СЫРЫХ байт тела
//   с сохранением исходного порядка, а HMAC считается над остатком байт.

use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha1::Sha1;
use subtle::ConstantTimeEq;

use super::provider::{PaymentProvider, PaymentWebhookAction};
use caramba_db::models::store::{PaymentSession, User};

pub struct PlisioProvider {
    /// API Key из личного кабинета Plisio
    pub api_key: String,
    pub api_domain: String,
    pub bot_username: String,
}

#[async_trait]
impl PaymentProvider for PlisioProvider {
    fn name(&self) -> &str {
        "plisio"
    }

    async fn create_invoice(
        &self,
        session: &PaymentSession,
        _user: &User,
        client: &reqwest::Client,
    ) -> Result<String> {
        let amount = format!("{:.2}", (session.amount as f64) / 100.0);
        let currency = session.currency.to_uppercase();
        // `json=true` ОБЯЗАТЕЛЕН: без него Plisio шлёт callback как PHP-serialize
        // form-data, и verify_hash невозможно воспроизвести из Rust. С этим
        // параметром callback приходит как JSON, а verify_hash считается над
        // JSON-телом (см. модульную доку выше).
        let callback_url = format!(
            "https://{}/api/webhooks/payment/plisio?json=true",
            self.api_domain
        );
        let success_url = format!("https://t.me/{}", self.bot_username);

        // Plisio поддерживает GET с query-параметрами для создания инвойса.
        let url = reqwest::Url::parse_with_params(
            "https://api.plisio.net/api/v1/invoices/new",
            &[
                ("api_key", self.api_key.as_str()),
                ("order_number", session.id.to_string().as_str()),
                ("source_currency", currency.as_str()),
                ("source_amount", amount.as_str()),
                ("currency", "USDT"),
                ("allowed_psys_cids", "BTC,ETH,USDT,LTC,TRX"),
                ("callback_url", callback_url.as_str()),
                ("success_callback_url", success_url.as_str()),
                (
                    "description",
                    &format!("VPN Subscription (Product: {})", session.product_id),
                ),
            ],
        )
        .context("Не удалось сформировать URL запроса Plisio")?;

        let res = client
            .get(url)
            .send()
            .await
            .context("Не удалось отправить запрос в Plisio")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Plisio API Error: {}", error_text);
        }

        let resp: Value = res
            .json()
            .await
            .context("Не удалось разобрать ответ Plisio")?;

        let status = resp.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "success" {
            anyhow::bail!("Plisio API вернул ошибку: {}", resp);
        }

        resp.pointer("/data/invoice_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Plisio не вернул invoice_url"))
    }

    async fn verify_webhook(&self, payload: &[u8], _signature: &str) -> Result<bool> {
        // Plisio встраивает verify_hash прямо в JSON-тело вебхука (требуется
        // json=true в callback_url, см. модульную доку выше). Подпись —
        // HMAC-SHA1 над JSON-телом БЕЗ поля verify_hash, в исходном порядке
        // вставки, ключ = api_key. Сравнение — в постоянном времени.

        // 1) Достаём присланный verify_hash (только для сравнения).
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке Plisio")?;
        let received_hash = match data.get("verify_hash").and_then(|v| v.as_str()) {
            Some(h) if !h.is_empty() => h.to_ascii_lowercase(),
            _ => return Ok(false),
        };

        // 2) Вырезаем член verify_hash из СЫРЫХ байт тела, сохраняя порядок
        //    остальных полей — именно над этими байтами Plisio считал HMAC.
        let stripped = match strip_verify_hash_member(payload) {
            Some(bytes) => bytes,
            None => return Ok(false),
        };

        // 3) HMAC-SHA1(stripped, key = api_key) и константное сравнение hex.
        type HmacSha1 = Hmac<Sha1>;
        let mut mac = HmacSha1::new_from_slice(self.api_key.as_bytes())
            .context("Неверный HMAC-ключ Plisio")?;
        mac.update(&stripped);
        let expected = hex::encode(mac.finalize().into_bytes());

        // ct_eq на срезах разной длины вернёт false без утечки содержимого.
        Ok(expected.as_bytes().ct_eq(received_hash.as_bytes()).into())
    }

    async fn handle_webhook(&self, payload: &[u8]) -> Result<PaymentWebhookAction> {
        let data: Value =
            serde_json::from_slice(payload).context("Неверный JSON в вебхуке Plisio")?;

        let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let order_number = data
            .get("order_number")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if order_number.is_empty() {
            return Ok(PaymentWebhookAction::Ignored);
        }

        match status {
            "completed" | "mismatch" => {
                // mismatch = оплата поступила, но в неверной сумме — для VPN принимаем.
                Ok(PaymentWebhookAction::Completed {
                    external_id: order_number.to_string(),
                })
            }
            "expired" | "cancelled" | "error" => Ok(PaymentWebhookAction::Failed {
                reason: status.to_string(),
            }),
            _ => Ok(PaymentWebhookAction::Pending),
        }
    }

    async fn check_status(
        &self,
        _session: &PaymentSession,
        _client: &reqwest::Client,
    ) -> Result<String> {
        Ok("pending".to_string())
    }
}

/// Удаляет верхнеуровневый член `"verify_hash":<value>` из СЫРЫХ байт JSON-тела,
/// сохраняя байты и порядок остальных полей и убирая осиротевшую запятую.
///
/// Plisio считает `verify_hash` как HMAC-SHA1 от `json_encode()` тела без этого
/// поля. PHP `json_encode` компактен (без пробелов), поэтому байты остальных
/// полей в присланном теле идентичны тем, над которыми считалась подпись. Значит
/// вырезание члена из сырых байт даёт байт-в-байт ту же строку, что хешировал
/// Plisio — без повторной сериализации (которая в этом проекте отсортировала бы
/// ключи, т.к. serde_json собран без feature `preserve_order`).
///
/// Возвращает `None`, если тело не является JSON-объектом или член не найден.
fn strip_verify_hash_member(payload: &[u8]) -> Option<Vec<u8>> {
    // Находим границы верхнеуровневого объекта `{ ... }`.
    let open = payload.iter().position(|&b| b == b'{')?;
    // Проверяем, что до '{' только пробельные байты (валидный JSON-объект).
    if payload[..open].iter().any(|b| !b.is_ascii_whitespace()) {
        return None;
    }

    // Сканируем члены ВЕРХНЕГО уровня, отслеживая строки/экранирование и
    // вложенность, чтобы корректно пропускать значения-объекты/массивы и
    // не натыкаться на подстроку "verify_hash" внутри вложенных данных.
    let bytes = payload;
    let n = bytes.len();
    let mut i = open + 1;

    // Пропуск пробелов.
    let skip_ws = |bytes: &[u8], mut p: usize| -> usize {
        while p < bytes.len() && bytes[p].is_ascii_whitespace() {
            p += 1;
        }
        p
    };
    // Читает строку JSON начиная с открывающей кавычки на позиции p.
    // Возвращает (содержимое_без_кавычек, индекс_после_закрывающей_кавычки).
    let read_string = |bytes: &[u8], p: usize| -> Option<(String, usize)> {
        if p >= bytes.len() || bytes[p] != b'"' {
            return None;
        }
        let mut q = p + 1;
        let mut out = Vec::new();
        while q < bytes.len() {
            match bytes[q] {
                b'\\' => {
                    // Экранированный символ — копируем как есть (для имени ключа
                    // нам достаточно простого сравнения, ключи Plisio без эскейпов).
                    out.push(bytes[q]);
                    q += 1;
                    if q < bytes.len() {
                        out.push(bytes[q]);
                        q += 1;
                    }
                }
                b'"' => {
                    let s = String::from_utf8(out).ok()?;
                    return Some((s, q + 1));
                }
                other => {
                    out.push(other);
                    q += 1;
                }
            }
        }
        None
    };
    // Пропускает любое JSON-значение начиная с позиции p, корректно проходя
    // строки, объекты и массивы (с учётом вложенности и экранирования).
    let skip_value = |bytes: &[u8], p: usize| -> Option<usize> {
        let mut q = skip_ws(bytes, p);
        if q >= bytes.len() {
            return None;
        }
        match bytes[q] {
            b'"' => {
                // строка
                let mut r = q + 1;
                while r < bytes.len() {
                    match bytes[r] {
                        b'\\' => r += 2,
                        b'"' => return Some(r + 1),
                        _ => r += 1,
                    }
                }
                None
            }
            b'{' | b'[' => {
                // структура — считаем глубину, игнорируя скобки внутри строк
                let mut depth = 0i32;
                let mut in_str = false;
                let mut r = q;
                while r < bytes.len() {
                    let c = bytes[r];
                    if in_str {
                        match c {
                            b'\\' => r += 2,
                            b'"' => {
                                in_str = false;
                                r += 1;
                            }
                            _ => r += 1,
                        }
                        continue;
                    }
                    match c {
                        b'"' => in_str = true,
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(r + 1);
                            }
                        }
                        _ => {}
                    }
                    r += 1;
                }
                None
            }
            _ => {
                // число / true / false / null — читаем до разделителя
                while q < bytes.len() && !matches!(bytes[q], b',' | b'}' | b']') {
                    q += 1;
                }
                Some(q)
            }
        }
    };

    loop {
        i = skip_ws(bytes, i);
        if i >= n {
            return None;
        }
        if bytes[i] == b'}' {
            // Достигли конца объекта, член verify_hash не найден.
            return None;
        }

        // Начало члена: запоминаем позицию (для вырезания вместе с запятой).
        let member_start = i;
        let (key, after_key) = read_string(bytes, i)?;
        let colon = skip_ws(bytes, after_key);
        if colon >= n || bytes[colon] != b':' {
            return None;
        }
        let value_end = skip_value(bytes, colon + 1)?;
        let after_value = skip_ws(bytes, value_end);
        if after_value >= n {
            return None;
        }
        // Следующий байт — либо ',' (есть ещё члены), либо '}' (последний член).
        let has_trailing_comma = bytes[after_value] == b',';
        let member_end = if has_trailing_comma {
            after_value + 1
        } else {
            after_value
        };

        if key == "verify_hash" {
            // Вырезаем член. Нужно убрать ровно одну запятую-разделитель, чтобы
            // результат остался валидным JSON и совпал с json_encode без поля.
            let mut out = Vec::with_capacity(n);
            if has_trailing_comma {
                // member: `"verify_hash":...,` — убираем член вместе с его запятой.
                out.extend_from_slice(&bytes[..member_start]);
                out.extend_from_slice(&bytes[member_end..]);
            } else {
                // Последний член: `...,"verify_hash":...` — нужно убрать
                // предшествующую запятую (если member не первый).
                // Находим конец предыдущего значимого байта перед member_start.
                let mut cut = member_start;
                // Сдвигаемся назад через пробелы.
                while cut > open + 1 && bytes[cut - 1].is_ascii_whitespace() {
                    cut -= 1;
                }
                if cut > open + 1 && bytes[cut - 1] == b',' {
                    cut -= 1; // убираем предыдущую запятую
                }
                out.extend_from_slice(&bytes[..cut]);
                out.extend_from_slice(&bytes[member_end..]);
            }
            return Some(out);
        }

        // Переходим к следующему члену.
        if has_trailing_comma {
            i = member_end;
        } else {
            // Это был последний член, а verify_hash так и не встретился.
            return None;
        }
    }
}
