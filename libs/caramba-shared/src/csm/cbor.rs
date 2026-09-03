//! Кодировщик строгого профиля CBOR из `03-WIRE.md` раздел 3.
//!
//! Профиль это подмножество, а не весь CBOR: только определённые длины,
//! только кратчайшие головы, только беззнаковые ключи в строго возрастающем
//! порядке, никаких тегов, чисел с плавающей точкой, null и отрицательных
//! чисел. Общая библиотека приняла бы то, что верификатор обязан отвергнуть,
//! поэтому кодировщик здесь свой: он физически не умеет выдать байты за
//! пределами профиля.
//!
//! Кодировщик, а не декодер: панель документы ПОДПИСЫВАЕТ. Проверяют их
//! клиентские верификаторы (Go и Dart), и оба сверены с общим корпусом
//! `apps/caramba-client/docs/protocol/05-TEST-VECTORS`. Гейт для этого файла
//! другой и более строгий: тест воспроизводит эталонные документы корпуса
//! байт в байт (`tests/csm_vectors.rs`).

use std::collections::BTreeMap;

/// Значение строгого профиля. Отрицательных чисел, `null`, тегов и float здесь
/// нет по построению: их нельзя выразить.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Беззнаковое целое, major type 0.
    Uint(u64),
    /// Строка байт, major type 2.
    Bytes(Vec<u8>),
    /// Текст UTF-8, major type 3. `String` в Rust уже гарантирует валидность.
    Text(String),
    /// Массив, major type 4.
    Array(Vec<Value>),
    /// Карта, major type 5. `BTreeMap` даёт возрастающий порядок ключей как
    /// свойство типа: нарушить C10 невозможно, а не «не нужно».
    Map(BTreeMap<u64, Value>),
    /// `true` / `false`, единственные разрешённые простые значения.
    Bool(bool),
}

impl Value {
    /// Карта из пар. Дубликат ключа невозможен: последний выигрывает на этапе
    /// сборки, до кодирования, и до байтов не доходит.
    pub fn map(pairs: impl IntoIterator<Item = (u64, Value)>) -> Value {
        Value::Map(pairs.into_iter().collect())
    }
}

/// Пишет голову: major type плюс кратчайший аргумент (правило C4).
fn head(out: &mut Vec<u8>, major: u8, arg: u64) {
    let m = major << 5;
    match arg {
        0..=23 => out.push(m | arg as u8),
        24..=0xFF => {
            out.push(m | 24);
            out.push(arg as u8);
        }
        0x100..=0xFFFF => {
            out.push(m | 25);
            out.extend_from_slice(&(arg as u16).to_be_bytes());
        }
        0x1_0000..=0xFFFF_FFFF => {
            out.push(m | 26);
            out.extend_from_slice(&(arg as u32).to_be_bytes());
        }
        _ => {
            out.push(m | 27);
            out.extend_from_slice(&arg.to_be_bytes());
        }
    }
}

/// Кодирует значение в байты профиля.
pub fn encode(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    write(&mut out, v);
    out
}

fn write(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Uint(n) => head(out, 0, *n),
        Value::Bytes(b) => {
            head(out, 2, b.len() as u64);
            out.extend_from_slice(b);
        }
        Value::Text(s) => {
            head(out, 3, s.len() as u64);
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(items) => {
            head(out, 4, items.len() as u64);
            for it in items {
                write(out, it);
            }
        }
        Value::Map(pairs) => {
            head(out, 5, pairs.len() as u64);
            // BTreeMap отдаёт ключи по возрастанию: C10 выполняется структурой.
            for (k, val) in pairs {
                head(out, 0, *k);
                write(out, val);
            }
        }
        Value::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
    }
}

/// Пределы профиля (`03-WIRE.md` 3.2). Кодировщик проверяет их перед выдачей:
/// панель не имеет права выпустить документ, который клиент обязан отвергнуть.
pub const MAX_DEPTH: usize = 6;
pub const MAX_MAP_PAIRS: usize = 64;
pub const MAX_ARRAY_ITEMS: usize = 512;
pub const MAX_TSTR_BYTES: usize = 256;
pub const MAX_BSTR_BYTES: usize = 3072;
pub const MAX_UINT: u64 = (1u64 << 53) - 1;

/// Ошибка выхода за пределы профиля.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitError {
    pub what: &'static str,
    pub limit: usize,
    pub got: usize,
}

impl std::fmt::Display for LimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "csm cbor: {} превышает предел {} (получено {})",
            self.what, self.limit, self.got
        )
    }
}

impl std::error::Error for LimitError {}

/// Проверяет значение против пределов профиля.
pub fn check(v: &Value) -> Result<(), LimitError> {
    check_at(v, 1)
}

fn check_at(v: &Value, depth: usize) -> Result<(), LimitError> {
    if depth > MAX_DEPTH {
        return Err(LimitError {
            what: "глубина вложенности",
            limit: MAX_DEPTH,
            got: depth,
        });
    }
    match v {
        Value::Uint(n) => {
            if *n > MAX_UINT {
                return Err(LimitError {
                    what: "значение uint",
                    limit: MAX_UINT as usize,
                    got: *n as usize,
                });
            }
        }
        Value::Bytes(b) => {
            if b.len() > MAX_BSTR_BYTES {
                return Err(LimitError {
                    what: "длина bstr",
                    limit: MAX_BSTR_BYTES,
                    got: b.len(),
                });
            }
        }
        Value::Text(s) => {
            if s.len() > MAX_TSTR_BYTES {
                return Err(LimitError {
                    what: "длина tstr",
                    limit: MAX_TSTR_BYTES,
                    got: s.len(),
                });
            }
        }
        Value::Array(items) => {
            if items.len() > MAX_ARRAY_ITEMS {
                return Err(LimitError {
                    what: "число элементов массива",
                    limit: MAX_ARRAY_ITEMS,
                    got: items.len(),
                });
            }
            for it in items {
                check_at(it, depth + 1)?;
            }
        }
        Value::Map(pairs) => {
            if pairs.len() > MAX_MAP_PAIRS {
                return Err(LimitError {
                    what: "число пар карты",
                    limit: MAX_MAP_PAIRS,
                    got: pairs.len(),
                });
            }
            for (k, val) in pairs {
                // Ключ 0 запрещён, 1024 и выше запрещены (3.3).
                if *k == 0 || *k >= 1024 {
                    return Err(LimitError {
                        what: "номер ключа карты",
                        limit: 1023,
                        got: *k as usize,
                    });
                }
                check_at(val, depth + 1)?;
            }
        }
        Value::Bool(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Проверенные байты из `03-WIRE.md` 3.4.
    #[test]
    fn worked_encodings_match_the_spec() {
        assert_eq!(encode(&Value::Uint(1)), vec![0x01]);
        assert_eq!(encode(&Value::Uint(443)), vec![0x19, 0x01, 0xbb]);
        assert_eq!(
            encode(&Value::Uint(1788307200)),
            vec![0x1a, 0x6a, 0x97, 0x67, 0x00]
        );
        assert_eq!(encode(&Value::Bool(true)), vec![0xf5]);
        assert_eq!(encode(&Value::Bool(false)), vec![0xf4]);
        assert_eq!(encode(&Value::Text("DE".into())), vec![0x62, 0x44, 0x45]);
        assert_eq!(encode(&Value::Bytes(vec![0u8; 8]))[0], 0x48);
        assert_eq!(encode(&Value::Bytes(vec![0u8; 32]))[..2], [0x58, 0x20]);
        assert_eq!(
            encode(&Value::map([(1, Value::Uint(1)), (2, Value::Uint(2))]))[0],
            0xa2
        );
        assert_eq!(encode(&Value::Array(vec![Value::Uint(1)]))[0], 0x81);
        // Ключ 24 стоит двух байт, ради чего горячие поля держат ниже 24.
        assert_eq!(
            encode(&Value::map([(24, Value::Uint(0))]))[1..3],
            [0x18, 0x18]
        );
    }

    #[test]
    fn map_keys_are_emitted_in_ascending_order() {
        let v = Value::map([
            (11, Value::Uint(0)),
            (3, Value::Uint(0)),
            (7, Value::Uint(0)),
        ]);
        let bytes = encode(&v);
        assert_eq!(bytes, vec![0xa3, 0x03, 0x00, 0x07, 0x00, 0x0b, 0x00]);
    }

    #[test]
    fn limits_are_enforced_before_emission() {
        assert!(check(&Value::Bytes(vec![0u8; MAX_BSTR_BYTES])).is_ok());
        assert!(check(&Value::Bytes(vec![0u8; MAX_BSTR_BYTES + 1])).is_err());
        assert!(check(&Value::map([(0, Value::Uint(1))])).is_err());
        assert!(check(&Value::map([(1024, Value::Uint(1))])).is_err());
    }
}
