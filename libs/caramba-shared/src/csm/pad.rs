//! Набивка на сетку 256 байт (`03-WIRE.md` раздел 12).
//!
//! Одна арифметика для всех типов документов: директива и её обёртка
//! набиваются на каждый запрос, каталог и его части один раз при подписи.
//! Держать два экземпляра правила корзин значило бы, что клиентские
//! наблюдения размера двух документов одного тенанта расходятся по разным
//! ошибкам округления.

use std::collections::BTreeMap;

use super::cbor::{self, Value};

/// Шаг сетки паддинга (`03-WIRE.md` 12.2).
pub const PAD_UNIT: usize = 256;

/// Ошибка набивки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PadError {
    /// В карте уже есть ключ 9: набивать дважды нельзя.
    AlreadyPadded,
    /// Выход за пределы профиля CBOR.
    Limit(cbor::LimitError),
    /// Даже без набивки кадр не помещается под потолок.
    Ceiling { frame_len: usize, ceiling: usize },
}

impl std::fmt::Display for PadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PadError::AlreadyPadded => write!(f, "csm pad: паддинг уже присутствует"),
            PadError::Limit(e) => write!(f, "csm pad: {e}"),
            PadError::Ceiling { frame_len, ceiling } => write!(
                f,
                "csm pad: кадр в {frame_len} байт не помещается под потолок {ceiling}"
            ),
        }
    }
}

impl std::error::Error for PadError {}

impl From<cbor::LimitError> for PadError {
    fn from(e: cbor::LimitError) -> Self {
        PadError::Limit(e)
    }
}

/// Число нулевых байт `pd`, при котором закодированное поле стоит ровно `d`
/// байт; `None` для недостижимых 1, 26 и 259 (`03-WIRE.md` 12.2).
fn pad_bytes_for(d: usize) -> Option<usize> {
    match d {
        0 => Some(0),
        2..=25 => Some(d - 2),
        27..=258 => Some(d - 3),
        260..=3076 => Some(d - 4),
        _ => None,
    }
}

/// Кладёт `pd` (ключ 9) так, чтобы кадр лёг на сетку `PAD_UNIT` плюс `bucket`
/// корзин, и зажимает `bucket` под `ceiling`: паддинг не имеет права сломать
/// потолок, который понесёт ответ. `map` это полезная нагрузка без `pd`,
/// `nsigs` число подписей будущего кадра.
pub fn pad_to_bucket(
    mut map: BTreeMap<u64, Value>,
    nsigs: usize,
    bucket: u32,
    ceiling: usize,
) -> Result<Vec<u8>, PadError> {
    if map.contains_key(&9) {
        return Err(PadError::AlreadyPadded);
    }
    let base = Value::Map(map.clone());
    cbor::check(&base)?;
    let base = cbor::encode(&base);
    let unpadded = 7 + base.len() + 1 + 76 * nsigs;
    let grid = unpadded.div_ceil(PAD_UNIT);

    let mut r = bucket as usize;
    loop {
        let mut target = PAD_UNIT * (grid + r);
        let mut n = pad_bytes_for(target - unpadded);
        if n.is_none() {
            target += PAD_UNIT;
            n = pad_bytes_for(target - unpadded);
        }
        if let Some(n) = n
            && target <= ceiling
        {
            if target == unpadded {
                return Ok(base);
            }
            map.insert(9, Value::Bytes(vec![0u8; n]));
            let v = Value::Map(map);
            cbor::check(&v)?;
            let out = cbor::encode(&v);
            debug_assert_eq!(7 + out.len() + 1 + 76 * nsigs, target);
            return Ok(out);
        }
        if r == 0 {
            return Err(PadError::Ceiling {
                frame_len: unpadded,
                ceiling,
            });
        }
        r -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_lands_on_the_grid_and_clamps() {
        let mut m = BTreeMap::new();
        m.insert(1, Value::Uint(1));
        // Пустая карта плюс одна пара: кадр 7 + 3 + 1 + 76 = 87 байт.
        let p = pad_to_bucket(m.clone(), 1, 0, 4096).unwrap();
        assert_eq!(7 + p.len() + 1 + 76, 256);
        let p = pad_to_bucket(m.clone(), 1, 3, 4096).unwrap();
        assert_eq!(7 + p.len() + 1 + 76, 1024);
        // Зажим: потолок 512 режет r = 3 до r = 1.
        let p = pad_to_bucket(m.clone(), 1, 3, 512).unwrap();
        assert_eq!(7 + p.len() + 1 + 76, 512);
        assert_eq!(
            pad_to_bucket(m, 1, 0, 80),
            Err(PadError::Ceiling {
                frame_len: 87,
                ceiling: 80
            })
        );
    }

    #[test]
    fn padding_steps_over_unreachable_distances() {
        // Кадр в 255 байт без pd: до 256 один байт, недостижимо, идём на 512.
        let mut m = BTreeMap::new();
        m.insert(1, Value::Bytes(vec![0xaa; 167]));
        let base = cbor::encode(&Value::Map(m.clone()));
        assert_eq!(7 + base.len() + 1 + 76, 255);
        let p = pad_to_bucket(m, 1, 0, 4096).unwrap();
        assert_eq!(7 + p.len() + 1 + 76, 512);
    }

    #[test]
    fn a_map_already_padded_is_refused() {
        let mut m = BTreeMap::new();
        m.insert(9, Value::Bytes(Vec::new()));
        assert_eq!(pad_to_bucket(m, 1, 0, 4096), Err(PadError::AlreadyPadded));
    }
}
