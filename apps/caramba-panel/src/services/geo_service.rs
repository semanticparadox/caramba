use maxminddb::{Reader, geoip2};
use reqwest;
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
    reader: Option<Arc<Reader<Vec<u8>>>>,
    cache: Arc<Mutex<HashMap<String, (GeoData, Instant)>>>,
}

#[derive(Deserialize)]
struct IpApiResponse {
    #[serde(rename = "countryCode")]
    country_code: String,
    lat: f64,
    lon: f64,
}

impl GeoService {
    pub fn new(db_path: Option<&str>) -> Self {
        let reader = if let Some(path) = db_path {
            match Reader::open_readfile(path) {
                Ok(r) => Some(Arc::new(r)),
                Err(e) => {
                    tracing::warn!("Failed to open GeoIP DB at {}: {}", path, e);
                    None
                }
            }
        } else {
            None
        };

        Self {
            reader,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn get_location(&self, ip: &str) -> Option<GeoData> {
        // 1. Check Cache
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some((data, ts)) = cache.get(ip) {
                if ts.elapsed() < Duration::from_secs(86400) {
                    return Some(data.clone());
                } else {
                    cache.remove(ip);
                }
            }
        }

        // 2. Check DB
        if let Some(reader) = &self.reader {
            // Parse IP
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

                    let data = GeoData {
                        country_code: country,
                        lat,
                        lon,
                    };

                    let mut cache = self.cache.lock().unwrap();
                    cache.insert(ip.to_string(), (data.clone(), Instant::now()));
                    return Some(data);
                }
            }
        }

        // 3. Fallback API
        // Only if not private IP
        if ip == "127.0.0.1" || ip == "::1" || ip == "0.0.0.0" {
            tracing::debug!("GeoIP: skipping API lookup for local/unset IP {}", ip);
            return None;
        }

        // Primary: ip-api.com (free, 45 req/min)
        let url = format!("http://ip-api.com/json/{}?fields=countryCode,lat,lon", ip);
        match reqwest::get(&url).await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<IpApiResponse>().await {
                    let data = GeoData {
                        country_code: json.country_code,
                        lat: json.lat,
                        lon: json.lon,
                    };
                    tracing::debug!("GeoIP: ip-api.com resolved {} → {}", ip, data.country_code);
                    let mut cache = self.cache.lock().unwrap();
                    cache.insert(ip.to_string(), (data.clone(), Instant::now()));
                    return Some(data);
                }
            }
            Err(e) => tracing::warn!("GeoIP: ip-api.com failed for {}: {}", ip, e),
        }

        // Secondary: ipinfo.io (free, 50k req/month)
        let url2 = format!("https://ipinfo.io/{}/json", ip);
        match reqwest::get(&url2).await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(cc) = json.get("country").and_then(|v| v.as_str()) {
                        let data = GeoData {
                            country_code: cc.to_uppercase(),
                            lat: 0.0,
                            lon: 0.0,
                        };
                        tracing::debug!("GeoIP: ipinfo.io resolved {} → {}", ip, data.country_code);
                        let mut cache = self.cache.lock().unwrap();
                        cache.insert(ip.to_string(), (data.clone(), Instant::now()));
                        return Some(data);
                    }
                }
            }
            Err(e) => tracing::warn!("GeoIP: ipinfo.io also failed for {}: {}", ip, e),
        }

        tracing::warn!("GeoIP: all lookups failed for {}", ip);
        None
    }
}
