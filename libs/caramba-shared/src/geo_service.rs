// Единый GeoService для всех крейтов Caramba.
// Цепочка: локальная MaxMind DB → ip-api.com → ipinfo.io.
// Кэш: 24 часа, in-memory, без внешних зависимостей.

#[cfg(feature = "geo")]
use maxminddb::{geoip2, Reader};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoData {
    pub country_code: String,
    pub lat: f64,
    pub lon: f64,
}

pub struct GeoService {
    #[cfg(feature = "geo")]
    reader: Option<Arc<Reader<Vec<u8>>>>,
    #[cfg(not(feature = "geo"))]
    _reader: (),
    cache: Arc<Mutex<HashMap<String, (GeoData, Instant)>>>,
    // Переиспользуемый HTTP-клиент — не создаётся заново на каждый запрос
    #[cfg(feature = "geo")]
    http_client: reqwest::Client,
}

#[cfg(feature = "geo")]
#[derive(Deserialize)]
struct IpApiResponse {
    #[serde(rename = "countryCode")]
    country_code: String,
    lat: f64,
    lon: f64,
}

impl GeoService {
    pub fn new(db_path: Option<&str>) -> Self {
        #[cfg(feature = "geo")]
        {
            let reader = if let Some(path) = db_path {
                match Reader::open_readfile(path) {
                    Ok(r) => Some(Arc::new(r)),
                    Err(e) => {
                        tracing::warn!("GeoIP: не удалось открыть БД {}: {}", path, e);
                        None
                    }
                }
            } else {
                None
            };

            let http_client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("GeoService: не удалось создать HTTP-клиент");

            Self {
                reader,
                cache: Arc::new(Mutex::new(HashMap::new())),
                http_client,
            }
        }

        #[cfg(not(feature = "geo"))]
        {
            let _ = db_path;
            Self {
                _reader: (),
                cache: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    pub async fn get_location(&self, ip: &str) -> Option<GeoData> {
        // 1. Кэш
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((data, ts)) = cache.get(ip) {
                if ts.elapsed() < Duration::from_secs(86400) {
                    return Some(data.clone());
                } else {
                    cache.remove(ip);
                }
            }
        }

        #[cfg(feature = "geo")]
        {
            // 2. MaxMind DB
            if let Some(reader) = &self.reader {
                if let Ok(ip_addr) = ip.parse::<std::net::IpAddr>() {
                    if let Ok(city) = reader.lookup::<geoip2::City>(ip_addr) {
                        let country = city
                            .country
                            .and_then(|c| c.iso_code)
                            .unwrap_or("XX")
                            .to_string();
                        let lat = city
                            .location
                            .as_ref()
                            .and_then(|l| l.latitude)
                            .unwrap_or(0.0);
                        let lon = city
                            .location
                            .as_ref()
                            .and_then(|l| l.longitude)
                            .unwrap_or(0.0);

                        let data = GeoData { country_code: country, lat, lon };
                        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
                        cache.insert(ip.to_string(), (data.clone(), Instant::now()));
                        return Some(data);
                    }
                }
            }

            // 3. Пропускаем локальные адреса перед HTTP-запросами
            if ip == "127.0.0.1" || ip == "::1" || ip == "0.0.0.0" {
                tracing::debug!("GeoIP: пропуск API-запроса для локального IP {}", ip);
                return None;
            }

            // 4. Основной: ip-api.com (бесплатно, 45 req/min)
            let url = format!("http://ip-api.com/json/{}?fields=countryCode,lat,lon", ip);
            match self.http_client.get(&url).send().await {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<IpApiResponse>().await {
                        let data = GeoData {
                            country_code: json.country_code,
                            lat: json.lat,
                            lon: json.lon,
                        };
                        tracing::debug!("GeoIP: ip-api.com → {} = {}", ip, data.country_code);
                        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
                        cache.insert(ip.to_string(), (data.clone(), Instant::now()));
                        return Some(data);
                    }
                }
                Err(e) => tracing::warn!("GeoIP: ip-api.com недоступен для {}: {}", ip, e),
            }

            // 5. Резерв: ipinfo.io (бесплатно, 50k req/month)
            let url2 = format!("https://ipinfo.io/{}/json", ip);
            match self.http_client.get(&url2).send().await {
                Ok(resp) => {
                    if let Ok(json) = resp.json::<serde_json::Value>().await {
                        if let Some(cc) = json.get("country").and_then(|v| v.as_str()) {
                            let data = GeoData {
                                country_code: cc.to_uppercase(),
                                lat: 0.0,
                                lon: 0.0,
                            };
                            tracing::debug!("GeoIP: ipinfo.io → {} = {}", ip, data.country_code);
                            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
                            cache.insert(ip.to_string(), (data.clone(), Instant::now()));
                            return Some(data);
                        }
                    }
                }
                Err(e) => tracing::warn!("GeoIP: ipinfo.io также недоступен для {}: {}", ip, e),
            }

            tracing::warn!("GeoIP: все источники недоступны для {}", ip);
        }

        None
    }
}
