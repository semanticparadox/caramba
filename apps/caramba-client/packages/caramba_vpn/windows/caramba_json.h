// caramba_json.h — minimal extractors for the two flat FFI JSON shapes.
//
// CarambaStatus / CarambaTraffic return small, flat JSON objects with a fixed,
// known key set (the channel contract). Rather than vendoring a full JSON
// library into the plugin, we extract exactly the scalar fields we forward to
// the EventChannels. This is intentionally narrow: it parses string and integer
// values for top-level keys only.

#ifndef FLUTTER_PLUGIN_CARAMBA_JSON_H_
#define FLUTTER_PLUGIN_CARAMBA_JSON_H_

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>

namespace caramba_vpn {
namespace json {

// FindKey returns the index just after the colon following "key", or
// std::string::npos if the key is absent.
inline size_t FindValuePos(const std::string& src, const std::string& key) {
  const std::string needle = "\"" + key + "\"";
  size_t k = src.find(needle);
  if (k == std::string::npos) {
    return std::string::npos;
  }
  size_t colon = src.find(':', k + needle.size());
  if (colon == std::string::npos) {
    return std::string::npos;
  }
  size_t p = colon + 1;
  while (p < src.size() &&
         (src[p] == ' ' || src[p] == '\t' || src[p] == '\n' || src[p] == '\r')) {
    ++p;
  }
  return p;
}

// GetString reads a top-level string field. Returns false if absent or null.
// Handles the common JSON string escapes that appear in detail text.
inline bool GetString(const std::string& src, const std::string& key,
                      std::string* out) {
  size_t p = FindValuePos(src, key);
  if (p == std::string::npos || p >= src.size()) {
    return false;
  }
  if (src.compare(p, 4, "null") == 0) {
    return false;
  }
  if (src[p] != '"') {
    return false;
  }
  ++p;  // skip opening quote
  std::string value;
  while (p < src.size() && src[p] != '"') {
    char c = src[p];
    if (c == '\\' && p + 1 < src.size()) {
      char n = src[p + 1];
      switch (n) {
        case 'n': value.push_back('\n'); break;
        case 't': value.push_back('\t'); break;
        case 'r': value.push_back('\r'); break;
        case '"': value.push_back('"'); break;
        case '\\': value.push_back('\\'); break;
        case '/': value.push_back('/'); break;
        default: value.push_back(n); break;
      }
      p += 2;
      continue;
    }
    value.push_back(c);
    ++p;
  }
  *out = value;
  return true;
}

// EscapeString renders a value as a JSON string literal, quotes included.
//
// The two ABI v3 request bodies built here interpolate a value that arrives on
// the MethodChannel unvalidated. Concatenated raw, one double quote closes the
// literal and the caller writes sibling fields of its own choosing into
// DeviceSignRequest or DeviceAgreeRequest: another rkv, a kdf_info_b64, a
// different message. Android builds the same JSON with JSONObject and Apple
// with JSONSerialization; this is the same guarantee, written out.
inline std::string EscapeString(const std::string& value) {
  std::string out;
  out.reserve(value.size() + 2);
  out.push_back('"');
  for (const char raw : value) {
    const unsigned char c = static_cast<unsigned char>(raw);
    switch (c) {
      case '"': out += "\\\""; break;
      case '\\': out += "\\\\"; break;
      case '\n': out += "\\n"; break;
      case '\r': out += "\\r"; break;
      case '\t': out += "\\t"; break;
      case '\b': out += "\\b"; break;
      case '\f': out += "\\f"; break;
      default:
        if (c < 0x20) {
          char buf[7];
          snprintf(buf, sizeof(buf), "\\u%04x", c);
          out += buf;
        } else {
          out.push_back(raw);
        }
        break;
    }
  }
  out.push_back('"');
  return out;
}

// GetInt reads a top-level integer field. Returns the parsed value, or
// fallback if the key is absent or non-numeric.
inline int64_t GetInt(const std::string& src, const std::string& key,
                      int64_t fallback) {
  size_t p = FindValuePos(src, key);
  if (p == std::string::npos || p >= src.size()) {
    return fallback;
  }
  bool neg = false;
  if (src[p] == '-') {
    neg = true;
    ++p;
  }
  if (p >= src.size() || src[p] < '0' || src[p] > '9') {
    return fallback;
  }
  int64_t v = 0;
  while (p < src.size() && src[p] >= '0' && src[p] <= '9') {
    v = v * 10 + (src[p] - '0');
    ++p;
  }
  return neg ? -v : v;
}

}  // namespace json
}  // namespace caramba_vpn

#endif  // FLUTTER_PLUGIN_CARAMBA_JSON_H_
