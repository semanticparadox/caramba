// caramba_json.h — minimal extractors for the two flat FFI JSON shapes (Linux).
//
// CarambaStatus / CarambaTraffic return small flat JSON objects with a fixed key
// set (the channel contract). We extract only the scalar fields forwarded to the
// EventChannels rather than vendoring a JSON parser. Top-level string and
// integer values only.

#ifndef FLUTTER_PLUGIN_CARAMBA_JSON_LINUX_H_
#define FLUTTER_PLUGIN_CARAMBA_JSON_LINUX_H_

#include <glib.h>
#include <stdint.h>
#include <string.h>

// caramba_json_value_pos returns a pointer just past the colon following "key",
// skipping whitespace, or NULL if the key is absent.
static inline const char* caramba_json_value_pos(const char* src,
                                                 const char* key) {
  if (src == NULL || key == NULL) {
    return NULL;
  }
  gchar* needle = g_strdup_printf("\"%s\"", key);
  const char* k = strstr(src, needle);
  g_free(needle);
  if (k == NULL) {
    return NULL;
  }
  const char* colon = strchr(k, ':');
  if (colon == NULL) {
    return NULL;
  }
  const char* p = colon + 1;
  while (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r') {
    ++p;
  }
  return p;
}

// caramba_json_get_string reads a top-level string field. Returns a newly
// allocated string (free with g_free) or NULL if absent or null.
static inline gchar* caramba_json_get_string(const char* src, const char* key) {
  const char* p = caramba_json_value_pos(src, key);
  if (p == NULL) {
    return NULL;
  }
  if (strncmp(p, "null", 4) == 0) {
    return NULL;
  }
  if (*p != '"') {
    return NULL;
  }
  ++p;  // opening quote
  GString* out = g_string_new(NULL);
  while (*p != '\0' && *p != '"') {
    if (*p == '\\' && *(p + 1) != '\0') {
      char n = *(p + 1);
      switch (n) {
        case 'n': g_string_append_c(out, '\n'); break;
        case 't': g_string_append_c(out, '\t'); break;
        case 'r': g_string_append_c(out, '\r'); break;
        case '"': g_string_append_c(out, '"'); break;
        case '\\': g_string_append_c(out, '\\'); break;
        case '/': g_string_append_c(out, '/'); break;
        default: g_string_append_c(out, n); break;
      }
      p += 2;
      continue;
    }
    g_string_append_c(out, *p);
    ++p;
  }
  return g_string_free(out, FALSE);
}

// caramba_json_get_int reads a top-level integer field, or fallback if absent.
static inline int64_t caramba_json_get_int(const char* src, const char* key,
                                           int64_t fallback) {
  const char* p = caramba_json_value_pos(src, key);
  if (p == NULL) {
    return fallback;
  }
  gboolean neg = FALSE;
  if (*p == '-') {
    neg = TRUE;
    ++p;
  }
  if (*p < '0' || *p > '9') {
    return fallback;
  }
  int64_t v = 0;
  while (*p >= '0' && *p <= '9') {
    v = v * 10 + (*p - '0');
    ++p;
  }
  return neg ? -v : v;
}

#endif  // FLUTTER_PLUGIN_CARAMBA_JSON_LINUX_H_
