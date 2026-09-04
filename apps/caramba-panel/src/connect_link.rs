//! Формат ссылки `caramba://connect?d=<armor>` — самоописывающееся приглашение
//! в панель для standalone-приложения Caramba Connect.
//!
//! ЗАЧЕМ ОН ВООБЩЕ ЕСТЬ. Раньше единственным входом в приложение был экран
//! «введите код приглашения». Кодов на живой панели ноль, а кнопка QR — заглушка,
//! поэтому экран нельзя было пройти вообще никому. Ссылка убирает ввод: человек
//! получает её от бота и открывает, а всё остальное (куда идти, как называется
//! оператор, до какого момента приглашение живо) приложение читает из неё самой.
//!
//! ЧЕСТНО О СВОЙСТВАХ. Это НЕПРОЗРАЧНАЯ и ЦЕЛОСТНОСТНО-ПРОВЕРЯЕМАЯ строка, а НЕ
//! шифртекст. Зашифровать её нечем и незачем: у приложения нет общего секрета с
//! оператором, которого оно ещё ни разу не видело, и адрес коннектора ему нужно
//! прочитать до какого-либо обращения к сети. Base32 здесь — транспортная броня
//! (ссылка переживает мессенджеры, регистр и переносы строк), а не тайна.
//! Конфиденциальность даёт способ доставки (личное сообщение бота) плюс то, что
//! код одноразовый и короткоживущий. Нигде — ни в комментариях, ни тем более в
//! тексте для человека — эту ссылку нельзя называть «зашифрованной».
//!
//! РАСКЛАДКА БАЙТ (её же парсит приложение, поэтому она нормативна):
//!
//! ```text
//! caramba://connect?d=<armor>
//!
//! <armor> = Crockford base32 (верхний регистр, без '='), от конкатенации:
//!
//!   offset 0..3    "CJ1"        3 байта, магия
//!   offset 3..4    version      1 байт, 0x01
//!   offset 4..4+N  payload      N байт, строгий профиль CBOR (карта)
//!   последние 4    checksum     sha256("CJ1" || version || payload)[0..4]
//!
//! payload — CBOR-карта строгого профиля CSM (ключи uint, строго возрастающие,
//! кратчайшие головы, без тегов/float/null):
//!
//!   1 => tstr   connector origin — https-origin панели, "https://app.exarobot.top"
//!   2 => bstr   code — 16 случайных байт, одноразовый секрет приглашения
//!   3 => tstr   operator name — для экрана подтверждения
//!   4 => bstr   root key id — 12 байт, ОПЦИОНАЛЬНО, только если оператор
//!               провёл церемонию ключей (csm_keys, роль root)
//!   5 => uint   expires at — unix-секунды
//! ```
//!
//! Кодировщик payload — общий с протоколом (`caramba_shared::csm::cbor`): панель
//! не имеет права выдать байты за пределами профиля. Декодер здесь свой и
//! маленький; он нужен тестам round-trip и служит эталоном для парсера в Dart —
//! если он что-то отвергает, приложение обязано отвергать это же.
//!
//! ПОЧЕМУ ВЫПУСК ФАЛЛИБЕЛЕН. Правило «панель не имеет права выдать документ,
//! который клиент обязан отвергнуть» не соблюдается само собой: имя оператора и
//! адрес панели приходят из настроек, где их пишет человек. Строка длиннее 256
//! байт даёт ссылку с сошедшейся контрольной суммой, которую КАЖДАЯ сборка
//! приложения обязана отвергнуть по пределу профиля, — а совет «попросите
//! оператора прислать новую» порождает ровно такую же ссылку. Поэтому
//! [`ConnectProfile::to_envelope`] сначала прогоняет payload через
//! [`cbor::check`] и возвращает ошибку оператору, а не тупик пользователю.
//!
//! Вторая проверка того же выпуска — печатность. Имя оператора это ЗАЯВЛЕНИЕ
//! отправителя, которое приложение рисует строкой рядом с адресом панели.
//! Перевод строки внутри имени в такой раскладке даёт вторую строку, визуально
//! неотличимую от настоящей («Адрес панели …»), а bidi-override переставляет
//! символы уже отрисованного текста. Ссылку может сминтить кто угодно — формат
//! опубликован, а хвост это контрольная сумма, не MAC, — поэтому парсер
//! приложения такие имена отвергает; здесь стоит зеркальный запрет, чтобы
//! панель не выпускала того, что её же клиент откажется открыть.

use caramba_shared::csm::cbor::{self, Value};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Схема и путь ссылки. Отдельной константой, потому что её же ищет приложение.
pub const LINK_PREFIX: &str = "caramba://connect?d=";

/// Магия конверта. Три байта, ASCII.
pub const MAGIC: [u8; 3] = *b"CJ1";

/// Версия конверта. Меняется только при несовместимой смене раскладки.
pub const VERSION: u8 = 0x01;

/// Длина одноразового секрета приглашения в байтах.
pub const CODE_LEN: usize = 16;

/// Длина хвостовой контрольной суммы.
const CHECKSUM_LEN: usize = 4;

/// Ключи карты payload.
const K_ORIGIN: u64 = 1;
const K_CODE: u64 = 2;
const K_OPERATOR: u64 = 3;
const K_ROOT_KID: u64 = 4;
const K_EXPIRES: u64 = 5;

/// Алфавит Crockford base32: без I, L, O и U — чтобы человек не мог перепутать
/// символ при чтении вслух или при ручном переносе.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Префикс строки `enrollment_codes.code` для кодов, выданных этой ссылкой.
///
/// Пространство имён, а не украшение: код в самой ссылке — 16 сырых байт, места
/// для префикса там нет, поэтому маркер живёт только в БД. Благодаря ему код
/// приглашения устройства физически не может совпасть с реферальным enroll-кодом
/// из другого источника, и погашение одного никогда не спишет другой. Схему
/// таблицы это не трогает (колонка и так TEXT).
pub const DB_CODE_PREFIX: &str = "lnk_";

/// Профиль приглашения — то, что лежит в payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectProfile {
    /// https-origin панели, к которой приложение пойдёт за всем остальным.
    pub connector_origin: String,
    /// Одноразовый секрет. В БД он хранится как `lnk_<hex>` (см. [`db_code`]).
    pub code: [u8; CODE_LEN],
    /// Имя оператора для экрана подтверждения.
    pub operator_name: String,
    /// Идентификатор корневого ключа, если протокол включён.
    pub root_key_id: Option<[u8; 12]>,
    /// Момент истечения приглашения, unix-секунды.
    pub expires_at: u64,
}

/// Причина, по которой строка не является валидной ссылкой. Разделена детально
/// намеренно: это диагностика для разработчика приложения, а не ответ в сеть.
/// Панель ссылки только ВЫПУСКАЕТ, поэтому разборная половина в проде не
/// вызывается. Она здесь не «на будущее»: это нормативный эталон, против
/// которого пишется парсер в приложении, и гейт для round-trip тестов — без неё
/// раскладка байт держалась бы на честном слове. Тот же приём, что и у
/// `translations::KEYS`.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// Строка не начинается с `caramba://connect?d=`.
    Scheme,
    /// В armor встретился символ вне алфавита Crockford.
    Armor,
    /// Хвостовые биты armor не нулевые — строка обрезана или дописана.
    ArmorPadding,
    /// Байт меньше, чем минимальный конверт.
    TooShort,
    /// Первые три байта не "CJ1".
    Magic,
    /// Неизвестная версия конверта.
    Version(u8),
    /// Контрольная сумма не сошлась: строка повреждена.
    Checksum,
    /// payload не соответствует строгому профилю CBOR.
    Cbor(&'static str),
    /// Профиль разобрался, но обязательное поле отсутствует или пустое.
    Field(&'static str),
    /// Выпуск: payload вышел за пределы строгого профиля (длина строки, число
    /// пар и т.д.). Возникает только на стороне панели — разбор такое поймал бы
    /// раньше, декодером.
    Limit(cbor::LimitError),
    /// Выпуск: текстовое поле несёт символы, которыми можно подделать вид
    /// экрана подтверждения (управляющие, разделители строк, bidi-override).
    /// Первое поле — имя поля, второе — кодовая точка нарушителя.
    Unprintable(&'static str, u32),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::Scheme => write!(f, "not a caramba://connect link"),
            LinkError::Armor => write!(f, "armor contains a character outside Crockford base32"),
            LinkError::ArmorPadding => write!(f, "armor has non-zero trailing bits"),
            LinkError::TooShort => write!(f, "envelope shorter than magic+version+checksum"),
            LinkError::Magic => write!(f, "bad magic, expected CJ1"),
            LinkError::Version(v) => write!(f, "unsupported envelope version {v}"),
            LinkError::Checksum => write!(f, "checksum mismatch, link is corrupted"),
            LinkError::Cbor(what) => write!(f, "payload is not strict-profile CBOR: {what}"),
            LinkError::Field(what) => write!(f, "profile field invalid: {what}"),
            LinkError::Limit(e) => write!(
                f,
                "refusing to issue a link the app is required to reject: {e}"
            ),
            LinkError::Unprintable(what, cp) => write!(
                f,
                "refusing to issue a link the app is required to reject: \
                 {what} contains U+{cp:04X}, which can forge a row on the \
                 confirmation screen"
            ),
        }
    }
}

impl std::error::Error for LinkError {}

/// Новый одноразовый секрет: 16 байт из CSPRNG (2^128 — перебор невозможен,
/// поэтому погашение не нуждается в rate limit, в отличие от 6-значного кода).
pub fn new_code() -> [u8; CODE_LEN] {
    let mut buf = [0u8; CODE_LEN];
    rand::rng().fill_bytes(&mut buf);
    buf
}

/// Проводной вид кода — 32 символа нижнего регистра hex. Именно эту строку
/// приложение кладёт в тело POST-запроса погашения; сырые байты по сети не ходят.
pub fn code_hex(code: &[u8; CODE_LEN]) -> String {
    hex::encode(code)
}

/// Строка, под которой код лежит в `enrollment_codes.code`.
pub fn db_code(code_hex: &str) -> String {
    format!("{DB_CODE_PREFIX}{code_hex}")
}

/// Проверяет, что строка похожа на проводной код приглашения устройства, ДО
/// удара по базе: ровно 32 hex-символа нижнего регистра.
pub fn is_wire_code(s: &str) -> bool {
    s.len() == CODE_LEN * 2
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ConnectProfile {
    /// Собирает payload строгого профиля CBOR.
    fn to_cbor(&self) -> Value {
        let mut pairs: Vec<(u64, Value)> = vec![
            (K_ORIGIN, Value::Text(self.connector_origin.clone())),
            (K_CODE, Value::Bytes(self.code.to_vec())),
            (K_OPERATOR, Value::Text(self.operator_name.clone())),
            (K_EXPIRES, Value::Uint(self.expires_at)),
        ];
        // Ключ 4 отсутствует целиком, когда церемонии ключей не было. Именно
        // отсутствует, а не «пустой bstr»: приложение отличает «оператор не
        // включал протокол» от «оператор включил, но ключ нулевой».
        if let Some(kid) = self.root_key_id {
            pairs.push((K_ROOT_KID, Value::Bytes(kid.to_vec())));
        }
        Value::map(pairs)
    }

    /// Байты конверта: магия, версия, payload, контрольная сумма.
    ///
    /// Фаллибельна намеренно: см. «ПОЧЕМУ ВЫПУСК ФАЛЛИБЕЛЕН» в шапке модуля.
    /// Проверка идёт ДО кодирования, чтобы наружу вообще не выходил конверт,
    /// который приложение обязано отвергнуть.
    pub fn to_envelope(&self) -> Result<Vec<u8>, LinkError> {
        // Порядок проверок: сначала печатность (у неё точный адрес нарушителя),
        // потом пределы профиля. Наоборот оператор с длинным именем, полным
        // управляющих символов, узнал бы только про длину.
        check_printable(&self.connector_origin, "connector origin")?;
        check_printable(&self.operator_name, "operator name")?;
        let value = self.to_cbor();
        cbor::check(&value).map_err(LinkError::Limit)?;

        let payload = cbor::encode(&value);
        let mut out = Vec::with_capacity(4 + payload.len() + CHECKSUM_LEN);
        out.extend_from_slice(&MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&payload);
        out.extend_from_slice(&checksum(&out));
        Ok(out)
    }

    /// Armor без схемы — на случай, если ссылку надо показать отдельно от URL.
    pub fn to_armor(&self) -> Result<String, LinkError> {
        Ok(base32_encode(&self.to_envelope()?))
    }

    /// Готовая ссылка `caramba://connect?d=...`.
    pub fn to_link(&self) -> Result<String, LinkError> {
        Ok(format!("{LINK_PREFIX}{}", self.to_armor()?))
    }

    /// Разбор ссылки целиком. Схема сверяется без учёта регистра только у самой
    /// схемы; armor нормализуется по правилам Crockford.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn parse_link(link: &str) -> Result<Self, LinkError> {
        let s = link.trim();
        let armor = s
            .strip_prefix(LINK_PREFIX)
            .or_else(|| {
                // Мессенджеры и почтовики иногда чинят регистр схемы.
                let lower = s.to_ascii_lowercase();
                lower
                    .starts_with(LINK_PREFIX)
                    .then(|| &s[LINK_PREFIX.len()..])
            })
            .ok_or(LinkError::Scheme)?;
        Self::parse_armor(armor)
    }

    /// Разбор одного armor. Порядок проверок — от дешёвых к дорогим, и
    /// контрольная сумма проверяется ДО разбора CBOR: испорченную строку нельзя
    /// подсовывать парсеру.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn parse_armor(armor: &str) -> Result<Self, LinkError> {
        let bytes = base32_decode(armor)?;
        if bytes.len() < MAGIC.len() + 1 + CHECKSUM_LEN {
            return Err(LinkError::TooShort);
        }
        if bytes[..MAGIC.len()] != MAGIC {
            return Err(LinkError::Magic);
        }
        let version = bytes[MAGIC.len()];
        if version != VERSION {
            return Err(LinkError::Version(version));
        }

        let split = bytes.len() - CHECKSUM_LEN;
        let (signed, tail) = bytes.split_at(split);
        if checksum(signed) != tail {
            return Err(LinkError::Checksum);
        }

        Self::from_cbor(&decode_map(&signed[MAGIC.len() + 1..])?)
    }

    /// Достаёт поля из разобранной карты.
    #[cfg_attr(not(test), allow(dead_code))]
    fn from_cbor(map: &std::collections::BTreeMap<u64, Value>) -> Result<Self, LinkError> {
        let connector_origin = match map.get(&K_ORIGIN) {
            Some(Value::Text(t)) if !t.is_empty() => t.clone(),
            _ => return Err(LinkError::Field("connector origin")),
        };
        let code = match map.get(&K_CODE) {
            Some(Value::Bytes(b)) if b.len() == CODE_LEN => {
                let mut out = [0u8; CODE_LEN];
                out.copy_from_slice(b);
                out
            }
            _ => return Err(LinkError::Field("code")),
        };
        let operator_name = match map.get(&K_OPERATOR) {
            Some(Value::Text(t)) => t.clone(),
            _ => return Err(LinkError::Field("operator name")),
        };
        let root_key_id = match map.get(&K_ROOT_KID) {
            None => None,
            Some(Value::Bytes(b)) if b.len() == 12 => {
                let mut out = [0u8; 12];
                out.copy_from_slice(b);
                Some(out)
            }
            _ => return Err(LinkError::Field("root key id")),
        };
        let expires_at = match map.get(&K_EXPIRES) {
            Some(Value::Uint(n)) => *n,
            _ => return Err(LinkError::Field("expires at")),
        };

        Ok(ConnectProfile {
            connector_origin,
            code,
            operator_name,
            root_key_id,
            expires_at,
        })
    }
}

/// Отвергает текстовое поле, которым можно подделать вид экрана подтверждения.
///
/// Список запретов НЕ произвольный, он ровно повторяет `connect_link.dart`:
///   * C0/C1 и DEL — сюда попадают `\n` и `\r`, из-за которых одно поле
///     превращается в несколько строк экрана;
///   * U+2028/U+2029 — те же переносы, но «юникодные»: Flutter ломает строку и
///     по ним тоже;
///   * U+200E/U+200F/U+061C и U+202A..U+202E, U+2066..U+2069 — управление
///     направлением письма: они переставляют уже отрисованный текст, поэтому
///     видимая строка перестаёт соответствовать байтам.
///
/// Молча вычищать нельзя: оператор, чьё имя тихо переписали, увидит у клиентов
/// не то, что вводил, и не узнает почему. Отказ адресный — с кодовой точкой.
fn check_printable(s: &str, what: &'static str) -> Result<(), LinkError> {
    for ch in s.chars() {
        let cp = ch as u32;
        let hostile = ch.is_control()
            || matches!(cp, 0x7f..=0x9f)
            || matches!(cp, 0x2028 | 0x2029)
            || matches!(cp, 0x200e | 0x200f | 0x061c)
            || matches!(cp, 0x202a..=0x202e)
            || matches!(cp, 0x2066..=0x2069);
        if hostile {
            return Err(LinkError::Unprintable(what, cp));
        }
    }
    Ok(())
}

/// Первые четыре байта sha256 от подписываемой части конверта.
fn checksum(signed: &[u8]) -> [u8; CHECKSUM_LEN] {
    let digest = Sha256::digest(signed);
    let mut out = [0u8; CHECKSUM_LEN];
    out.copy_from_slice(&digest[..CHECKSUM_LEN]);
    out
}

// =============================================================================
// Crockford base32
// =============================================================================

/// Кодирует байты в Crockford base32 верхнего регистра без выравнивания.
/// Биты идут старшими вперёд; последняя неполная группа добивается нулями.
fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &b in data {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let idx = ((acc >> bits) & 0x1f) as usize;
            out.push(ALPHABET[idx] as char);
        }
    }
    if bits > 0 {
        let idx = ((acc << (5 - bits)) & 0x1f) as usize;
        out.push(ALPHABET[idx] as char);
    }
    out
}

/// Обратное преобразование с нормализацией Crockford: регистр не важен, I и L
/// читаются как 1, O как 0, дефисы-разделители игнорируются. Хвостовые биты
/// обязаны быть нулевыми — иначе строку дописали или обрезали, и молча
/// принимать её нельзя.
#[cfg_attr(not(test), allow(dead_code))]
fn base32_decode(s: &str) -> Result<Vec<u8>, LinkError> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8 + 1);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for ch in s.chars() {
        if ch == '-' {
            continue;
        }
        let up = ch.to_ascii_uppercase();
        let v = match up {
            'O' => 0u8,
            'I' | 'L' => 1u8,
            _ => match ALPHABET.iter().position(|&a| a as char == up) {
                Some(i) => i as u8,
                None => return Err(LinkError::Armor),
            },
        };
        acc = (acc << 5) | u32::from(v);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    if bits > 0 && (acc & ((1 << bits) - 1)) != 0 {
        return Err(LinkError::ArmorPadding);
    }
    Ok(out)
}

// =============================================================================
// Минимальный декодер строгого профиля CBOR
// =============================================================================

/// Разбирает ровно одну карту строгого профиля и требует, чтобы она занимала
/// весь буфер: висящий хвост означает подделку или обрезку.
#[cfg_attr(not(test), allow(dead_code))]
fn decode_map(buf: &[u8]) -> Result<std::collections::BTreeMap<u64, Value>, LinkError> {
    let mut r = Reader { buf, pos: 0 };
    let v = r.value(0)?;
    if r.pos != buf.len() {
        return Err(LinkError::Cbor("trailing bytes after payload"));
    }
    match v {
        Value::Map(m) => Ok(m),
        _ => Err(LinkError::Cbor("payload root is not a map")),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

#[cfg_attr(not(test), allow(dead_code))]
impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, LinkError> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or(LinkError::Cbor("unexpected end"))?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], LinkError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(LinkError::Cbor("length overflow"))?;
        let s = self
            .buf
            .get(self.pos..end)
            .ok_or(LinkError::Cbor("unexpected end"))?;
        self.pos = end;
        Ok(s)
    }

    /// Голова: major type и аргумент. Профиль требует КРАТЧАЙШЕЙ формы, поэтому
    /// длинная кодировка малого числа отвергается — иначе один и тот же профиль
    /// имел бы два разных представления, и контрольная сумма перестала бы
    /// однозначно отвечать за содержимое.
    fn head(&mut self) -> Result<(u8, u64), LinkError> {
        let b = self.byte()?;
        let major = b >> 5;
        let info = b & 0x1f;
        let arg = match info {
            0..=23 => u64::from(info),
            24 => {
                let v = u64::from(self.byte()?);
                if v < 24 {
                    return Err(LinkError::Cbor("non-shortest head"));
                }
                v
            }
            25 => {
                let s = self.take(2)?;
                let v = u64::from(u16::from_be_bytes([s[0], s[1]]));
                if v <= 0xFF {
                    return Err(LinkError::Cbor("non-shortest head"));
                }
                v
            }
            26 => {
                let s = self.take(4)?;
                let v = u64::from(u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
                if v <= 0xFFFF {
                    return Err(LinkError::Cbor("non-shortest head"));
                }
                v
            }
            27 => {
                let s = self.take(8)?;
                let v = u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
                if v <= 0xFFFF_FFFF {
                    return Err(LinkError::Cbor("non-shortest head"));
                }
                v
            }
            _ => return Err(LinkError::Cbor("reserved additional info")),
        };
        Ok((major, arg))
    }

    fn value(&mut self, depth: usize) -> Result<Value, LinkError> {
        if depth > cbor::MAX_DEPTH {
            return Err(LinkError::Cbor("nesting too deep"));
        }
        let b = *self
            .buf
            .get(self.pos)
            .ok_or(LinkError::Cbor("unexpected end"))?;
        // Простые значения (true/false) не имеют аргумента и разбираются отдельно.
        if b == 0xf4 || b == 0xf5 {
            self.pos += 1;
            return Ok(Value::Bool(b == 0xf5));
        }
        let (major, arg) = self.head()?;
        match major {
            0 => Ok(Value::Uint(arg)),
            2 => {
                let n = usize::try_from(arg).map_err(|_| LinkError::Cbor("bstr too long"))?;
                Ok(Value::Bytes(self.take(n)?.to_vec()))
            }
            3 => {
                let n = usize::try_from(arg).map_err(|_| LinkError::Cbor("tstr too long"))?;
                let s = std::str::from_utf8(self.take(n)?)
                    .map_err(|_| LinkError::Cbor("tstr is not valid UTF-8"))?;
                Ok(Value::Text(s.to_string()))
            }
            4 => {
                let n = usize::try_from(arg).map_err(|_| LinkError::Cbor("array too long"))?;
                let mut items = Vec::with_capacity(n.min(cbor::MAX_ARRAY_ITEMS));
                for _ in 0..n {
                    items.push(self.value(depth + 1)?);
                }
                Ok(Value::Array(items))
            }
            5 => {
                let n = usize::try_from(arg).map_err(|_| LinkError::Cbor("map too large"))?;
                let mut map = std::collections::BTreeMap::new();
                let mut prev: Option<u64> = None;
                for _ in 0..n {
                    let (km, key) = self.head()?;
                    if km != 0 {
                        return Err(LinkError::Cbor("map key is not an unsigned integer"));
                    }
                    // Строго возрастающий порядок — правило профиля C10. Оно же
                    // делает дубликат ключа невыразимым.
                    if prev.is_some_and(|p| key <= p) {
                        return Err(LinkError::Cbor("map keys not strictly increasing"));
                    }
                    prev = Some(key);
                    let val = self.value(depth + 1)?;
                    map.insert(key, val);
                }
                Ok(Value::Map(map))
            }
            _ => Err(LinkError::Cbor("major type outside the strict profile")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Фиксированный профиль: на нём же строится опубликованный пример ссылки,
    /// против которого пишется парсер в приложении.
    fn sample() -> ConnectProfile {
        ConnectProfile {
            connector_origin: "https://app.exarobot.top".to_string(),
            // 000102...0f — намеренно предсказуемый код, чтобы пример был
            // воспроизводим побайтно.
            code: [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f,
            ],
            operator_name: "Caramba Connect".to_string(),
            root_key_id: None,
            expires_at: 1_780_000_000,
        }
    }

    #[test]
    fn armor_round_trip() {
        let p = sample();
        let armor = p.to_armor().expect("sample must be issuable");
        let parsed = ConnectProfile::parse_armor(&armor).expect("armor must parse");
        assert_eq!(parsed, p);
    }

    #[test]
    fn link_round_trip_with_root_key() {
        let mut p = sample();
        p.root_key_id = Some([0xaa; 12]);
        let link = p.to_link().expect("sample must be issuable");
        let parsed = ConnectProfile::parse_link(&link).expect("link must parse");
        assert_eq!(parsed, p);
        assert_eq!(parsed.root_key_id, Some([0xaa; 12]));
    }

    /// Печатает эталонную ссылку и её конверт в hex. Гейт не в `assert`, а в том,
    /// что напечатанное совпадает с опубликованным в отчёте: парсер приложения
    /// пишется против этих самых байт.
    #[test]
    fn golden_example_is_stable() {
        let p = sample();
        let env = p.to_envelope().expect("sample must be issuable");
        assert_eq!(&env[..3], b"CJ1");
        assert_eq!(env[3], VERSION);
        // Раскладка зафиксирована: изменение любого байта — несовместимая смена
        // формата и обязано ломать этот тест.
        assert_eq!(
            hex::encode(&env),
            concat!(
                "434a3101",
                "a4",
                "017818",
                "68747470733a2f2f6170702e657861726f626f742e746f70",
                "0250",
                "000102030405060708090a0b0c0d0e0f",
                "036f",
                "436172616d626120436f6e6e656374",
                "051a6a18a500",
                "00c3cc73",
            )
        );
        // Опубликованный в отчёте пример. Парсер приложения пишется против
        // именно этой строки, поэтому её дрейф обязан ломать сборку панели.
        assert_eq!(
            p.to_link().expect("sample must be issuable"),
            "caramba://connect?d=8D5320D405W1GT3MEHR76EHF5XGQ0W1ECNW62WKFC9QQ8BKMDXR04M0004106\
1050R3GG28A1C60T3GF0DQM6RBJC5PP4R908DQPWVK5CDT0A6KA32JG0063SHSG"
        );
    }

    #[test]
    fn corrupted_checksum_is_rejected() {
        let p = sample();
        let mut env = p.to_envelope().expect("sample must be issuable");
        // Правим последний байт контрольной суммы — единственное отличие от
        // валидного конверта.
        let last = env.len() - 1;
        env[last] ^= 0x01;
        let armor = base32_encode(&env);
        assert_eq!(
            ConnectProfile::parse_armor(&armor).unwrap_err(),
            LinkError::Checksum
        );
    }

    #[test]
    fn corrupted_payload_is_rejected_before_parsing() {
        let p = sample();
        let mut env = p.to_envelope().expect("sample must be issuable");
        // Портим байт внутри payload: контрольная сумма обязана поймать это
        // раньше, чем декодер CBOR увидит мусор.
        env[10] ^= 0xff;
        assert_eq!(
            ConnectProfile::parse_armor(&base32_encode(&env)).unwrap_err(),
            LinkError::Checksum
        );
    }

    #[test]
    fn magic_and_version_are_checked() {
        let p = sample();
        let mut env = p.to_envelope().expect("sample must be issuable");
        env[0] = b'X';
        let cut = env.len() - CHECKSUM_LEN;
        let fixed = checksum(&env[..cut]);
        env[cut..].copy_from_slice(&fixed);
        assert_eq!(
            ConnectProfile::parse_armor(&base32_encode(&env)).unwrap_err(),
            LinkError::Magic
        );

        let mut env = p.to_envelope().expect("sample must be issuable");
        env[3] = 0x02;
        let cut = env.len() - CHECKSUM_LEN;
        let fixed = checksum(&env[..cut]);
        env[cut..].copy_from_slice(&fixed);
        assert_eq!(
            ConnectProfile::parse_armor(&base32_encode(&env)).unwrap_err(),
            LinkError::Version(2)
        );
    }

    #[test]
    fn crockford_normalisation_survives_transport() {
        let p = sample();
        let armor = p.to_armor().expect("sample must be issuable");
        // Нижний регистр и дефисы-переносы — то, что делают мессенджеры.
        let mangled = armor
            .to_lowercase()
            .chars()
            .collect::<Vec<_>>()
            .chunks(8)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("-");
        assert_eq!(ConnectProfile::parse_armor(&mangled).unwrap(), p);
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(
            ConnectProfile::parse_link("https://example.com").unwrap_err(),
            LinkError::Scheme
        );
        // 'U' исключена из алфавита Crockford.
        assert_eq!(
            ConnectProfile::parse_armor("UUUU").unwrap_err(),
            LinkError::Armor
        );
        assert_eq!(
            ConnectProfile::parse_armor("00000000").unwrap_err(),
            LinkError::TooShort
        );
    }

    /// Имя оператора длиннее предела профиля НЕ должно превращаться в ссылку.
    ///
    /// Без этого гейта выпуск проходил: конверт собирался, контрольная сумма
    /// сходилась, ссылка выглядела рабочей — и КАЖДАЯ сборка приложения обязана
    /// была отвергнуть её по MAX_TSTR_BYTES, посоветовав «попросите оператора
    /// прислать новую». Новая получалась ровно такой же. Ошибка обязана
    /// возникать здесь, у того, кто может её исправить.
    #[test]
    fn over_long_field_is_refused_at_issuance() {
        let mut p = sample();
        p.operator_name = "я".repeat(cbor::MAX_TSTR_BYTES); // 2 байта на символ
        assert!(p.operator_name.len() > cbor::MAX_TSTR_BYTES);
        assert_eq!(
            p.to_link().unwrap_err(),
            LinkError::Limit(cbor::LimitError {
                what: "длина tstr",
                limit: cbor::MAX_TSTR_BYTES,
                got: p.operator_name.len(),
            })
        );

        // Ровно на пределе ссылка обязана выпускаться: гейт отсекает выход за
        // профиль, а не «длинные имена вообще».
        let mut edge = sample();
        edge.operator_name = "n".repeat(cbor::MAX_TSTR_BYTES);
        let link = edge.to_link().expect("256 bytes is inside the profile");
        assert_eq!(
            ConnectProfile::parse_link(&link).unwrap().operator_name,
            edge.operator_name
        );

        // Адрес панели проверяется тем же гейтом: он приходит из тех же
        // настроек и точно так же пишется руками.
        let mut long_origin = sample();
        long_origin.connector_origin =
            format!("https://{}.example", "a".repeat(cbor::MAX_TSTR_BYTES));
        assert!(matches!(
            long_origin.to_link().unwrap_err(),
            LinkError::Limit(_)
        ));
    }

    /// Перевод строки и bidi-override в имени оператора — отказ на выпуске.
    ///
    /// Ровно эта строка на экране подтверждения рисует вторую строку «Адрес
    /// панели https://app.exarobot.top», визуально неотличимую от настоящей.
    #[test]
    fn unprintable_operator_name_is_refused_at_issuance() {
        let forged = "Caramba Connect\nАдрес панели   https://app.exarobot.top";
        let mut p = sample();
        p.operator_name = forged.to_string();
        assert_eq!(
            p.to_link().unwrap_err(),
            LinkError::Unprintable("operator name", 0x0a)
        );

        for (label, ch) in [
            ("CR", '\r'),
            ("NEL", '\u{85}'),
            ("LINE SEPARATOR", '\u{2028}'),
            ("PARAGRAPH SEPARATOR", '\u{2029}'),
            ("RLO", '\u{202e}'),
            ("RLI", '\u{2067}'),
            ("RLM", '\u{200f}'),
            ("ALM", '\u{061c}'),
            ("NUL", '\0'),
        ] {
            let mut p = sample();
            p.operator_name = format!("Caramba{ch}Connect");
            assert_eq!(
                p.to_link().unwrap_err(),
                LinkError::Unprintable("operator name", ch as u32),
                "{label} must not survive issuance"
            );
        }

        // Обычное имя с юникодом, эмодзи и пробелами проходит: гейт про
        // управляющие символы, а не про алфавит.
        let mut ok = sample();
        ok.operator_name = "Караmba «Connect» 🛡 — оператор".to_string();
        let link = ok.to_link().expect("a normal name must still be issuable");
        assert_eq!(
            ConnectProfile::parse_link(&link).unwrap().operator_name,
            ok.operator_name
        );
    }

    #[test]
    fn wire_code_shape() {
        let code = new_code();
        let h = code_hex(&code);
        assert!(is_wire_code(&h));
        assert_eq!(db_code(&h), format!("lnk_{h}"));
        // Верхний регистр и нестандартная длина — не наш проводной код.
        assert!(!is_wire_code(&h.to_uppercase()));
        assert!(!is_wire_code("abc"));
    }
}
