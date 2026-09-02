// caramba_vpn_plugin.cc — Linux (C++/GObject) implementation of the exarobot
// VPN plugin.
//
// Registers the com.caramba/vpn MethodChannel + status/traffic EventChannels,
// dlopens libcaramba_core.so (the cgo Go engine running mihomo with a kernel
// tun device), and drives stage + ~1 Hz traffic events from the FFI. mihomo
// owns the TUN on desktop, so we pass tunFd = -1 and never establish an fd.
//
// The 1 Hz poll runs on the GLib main loop (g_timeout), so EventChannel sends
// happen on the platform thread — no extra synchronization needed. The host
// process needs root or CAP_NET_ADMIN to create the tun device (pkexec /
// setcap) — see INTEGRATION.md.

#include "include/caramba_vpn/caramba_vpn_plugin.h"

#include <flutter_linux/flutter_linux.h>

#include "caramba_core_ffi.h"
#include "caramba_json.h"

#define CARAMBA_METHOD_CHANNEL "com.caramba/vpn"
#define CARAMBA_STATUS_CHANNEL "com.caramba/vpn/status"
#define CARAMBA_TRAFFIC_CHANNEL "com.caramba/vpn/traffic"

struct _CarambaVpnPlugin {
  GObject parent_instance;

  FlMethodChannel* method_channel;
  FlEventChannel* status_channel;
  FlEventChannel* traffic_channel;

  // EventChannel sinks become live once Dart subscribes; we hold the channels
  // and send through them while listening_* is TRUE.
  gboolean status_listening;
  gboolean traffic_listening;

  CarambaCoreFfi ffi;
  CarambaHandle handle;

  guint poll_source_id;  // g_timeout source, 0 when stopped.
  gchar* last_stage;     // owned; mirrors the last emitted stage string.
  gchar* last_detail;    // owned; mirrors the last emitted detail (may be NULL).
  int64_t last_connected_since_ms;  // mirrors the last emitted connectedSinceMs.

  // Auth/config seam captured from configure(); applied in ensure_core. Owned.
  gchar* panel_url;
  gchar* subscription_id;
  gchar* access_token;
  gboolean configured;

  // ABI v2: политика и способ захвата, заданные до создания ядра. Применяются
  // в ensure_core (и сразу, если ядро уже есть).
  gchar* policy_json;   // owned; NULL/"" — политика не задавалась
  gchar* tunnel_mode;   // owned; NULL/"" — режим по умолчанию (tun)
  int mixed_port;
};

G_DEFINE_TYPE(CarambaVpnPlugin, caramba_vpn_plugin, g_object_get_type())

// --- helpers -----------------------------------------------------------------

// take_string copies an FFI-owned C string and frees it via CarambaFreeString.
// Returns a newly allocated string (free with g_free), or NULL.
static gchar* take_string(CarambaVpnPlugin* self, char* s) {
  if (s == NULL) {
    return NULL;
  }
  gchar* out = g_strdup(s);
  if (self->ffi.FreeString != NULL) {
    self->ffi.FreeString(s);
  }
  return out;
}

// drop_string frees an FFI-owned string we do not need to read (e.g. the NULL
// or error result of Configure/SetTunFd/Down). Safe with NULL.
static void drop_string(CarambaVpnPlugin* self, char* s) {
  if (s != NULL && self->ffi.FreeString != NULL) {
    self->ffi.FreeString(s);
  }
}

// build_status_map produces the channel-contract status map.
static FlValue* build_status_map(const char* stage, const char* detail,
                                 int64_t connected_since_ms) {
  FlValue* map = fl_value_new_map();
  fl_value_set_string_take(map, "stage", fl_value_new_string(stage));
  if (detail != NULL) {
    fl_value_set_string_take(map, "detail", fl_value_new_string(detail));
  } else {
    fl_value_set_string_take(map, "detail", fl_value_new_null());
  }
  fl_value_set_string_take(map, "connectedSinceMs",
                           fl_value_new_int(connected_since_ms));
  return map;
}

// emit_stage sends a synthetic stage onto the status channel (for connecting /
// error states the core has not surfaced yet) and records last_stage.
static void emit_stage(CarambaVpnPlugin* self, const char* stage,
                       const char* detail) {
  g_clear_pointer(&self->last_stage, g_free);
  g_clear_pointer(&self->last_detail, g_free);
  self->last_stage = g_strdup(stage);
  self->last_detail = detail != NULL ? g_strdup(detail) : NULL;
  // Синтетические стадии (connecting/error) не несут connectedSinceMs — обнуляем,
  // чтобы status()/replay не отдавали устаревшее значение от прежнего сеанса.
  self->last_connected_since_ms = 0;
  if (self->status_listening && self->status_channel != NULL) {
    g_autoptr(FlValue) map = build_status_map(stage, detail, 0);
    fl_event_channel_send(self->status_channel, map, NULL, NULL);
  }
}

// apply_policy пушит накопленный CorePolicy JSON в ядро (ABI v2). No-op, если
// политики нет, ядро не создано, или символа нет в библиотеке.
static void apply_policy(CarambaVpnPlugin* self) {
  if (self->handle == 0 || self->ffi.SetPolicy == NULL) {
    return;
  }
  if (self->policy_json == NULL || self->policy_json[0] == '\0') {
    return;
  }
  drop_string(self, self->ffi.SetPolicy(self->handle, self->policy_json));
}

// apply_tunnel_mode пушит способ захвата трафика (ABI v2). No-op при тех же
// условиях, что и apply_policy.
static void apply_tunnel_mode(CarambaVpnPlugin* self) {
  if (self->handle == 0 || self->ffi.SetTunnelMode == NULL) {
    return;
  }
  if (self->tunnel_mode == NULL || self->tunnel_mode[0] == '\0') {
    return;
  }
  drop_string(self, self->ffi.SetTunnelMode(self->handle, self->tunnel_mode,
                                            self->mixed_port));
}

// ensure_core dlopens libcaramba_core.so and creates the handle. Emits an error
// stage and returns FALSE on failure.
static gboolean ensure_core(CarambaVpnPlugin* self) {
  if (self->handle != 0) {
    return TRUE;
  }
  if (!caramba_core_ffi_load(&self->ffi)) {
    emit_stage(self, "error", "exarobot core library not found");
    return FALSE;
  }
  // panelURL seeds the core; subURL/workDir/tokenPath default inside the core
  // when empty. The live session (subscription UUID + JWT) is applied via
  // CarambaConfigure below.
  const char* panel = self->panel_url != NULL ? self->panel_url : "";
  self->handle = self->ffi.New(panel, "", "", "");
  if (self->handle == 0) {
    emit_stage(self, "error", "exarobot core init failed");
    return FALSE;
  }
  if (!self->configured && self->ffi.Configure != NULL &&
      ((self->panel_url != NULL && self->panel_url[0] != '\0') ||
       (self->access_token != NULL && self->access_token[0] != '\0'))) {
    drop_string(self, self->ffi.Configure(
                          self->handle,
                          self->panel_url != NULL ? self->panel_url : "",
                          self->subscription_id != NULL ? self->subscription_id
                                                        : "",
                          self->access_token != NULL ? self->access_token
                                                     : ""));
    self->configured = TRUE;
  }
  // ABI v2: политика и режим захвата применяются сразу после создания ядра,
  // до любого Up. Отсутствующий символ (старый бинарь) молча пропускаем —
  // ошибку покажет отдельный вызов setPolicy/setTunnelMode из Dart.
  apply_policy(self);
  apply_tunnel_mode(self);
  return TRUE;
}

// caramba_configure stores the auth/config seam and pushes it into the core if
// it already exists (re-configure after a token refresh).
static void caramba_configure(CarambaVpnPlugin* self, const gchar* panel_url,
                              const gchar* subscription_id,
                              const gchar* access_token) {
  g_clear_pointer(&self->panel_url, g_free);
  g_clear_pointer(&self->subscription_id, g_free);
  g_clear_pointer(&self->access_token, g_free);
  self->panel_url = g_strdup(panel_url != NULL ? panel_url : "");
  self->subscription_id =
      g_strdup(subscription_id != NULL ? subscription_id : "");
  self->access_token = g_strdup(access_token != NULL ? access_token : "");
  self->configured = FALSE;
  if (self->handle != 0 && self->ffi.Configure != NULL) {
    drop_string(self, self->ffi.Configure(self->handle, self->panel_url,
                                          self->subscription_id,
                                          self->access_token));
    self->configured = TRUE;
  }
}

// poll_tick runs ~1 Hz on the GLib main loop while connected: forwards the FFI
// status + traffic JSON onto the EventChannels.
static gboolean poll_tick(gpointer user_data) {
  CarambaVpnPlugin* self = CARAMBA_VPN_PLUGIN(user_data);
  if (self->handle == 0 || self->ffi.Status == NULL) {
    return G_SOURCE_CONTINUE;
  }

  // Status.
  gchar* status_json = take_string(self, self->ffi.Status(self->handle));
  if (status_json != NULL) {
    gchar* stage = caramba_json_get_string(status_json, "stage");
    if (stage == NULL) {
      stage = g_strdup("connecting");
    }
    gchar* detail = caramba_json_get_string(status_json, "detail");
    int64_t since = caramba_json_get_int(status_json, "connectedSinceMs", 0);

    g_clear_pointer(&self->last_stage, g_free);
    g_clear_pointer(&self->last_detail, g_free);
    self->last_stage = g_strdup(stage);
    self->last_detail = detail != NULL ? g_strdup(detail) : NULL;
    self->last_connected_since_ms = since;

    if (self->status_listening && self->status_channel != NULL) {
      g_autoptr(FlValue) map = build_status_map(stage, detail, since);
      fl_event_channel_send(self->status_channel, map, NULL, NULL);
    }

    // Ядро само сообщило терминальную стадию (self-drop / фатальная ошибка) —
    // останавливаем опрос, иначе poll_tick будет крутиться вечно после разрыва.
    gboolean terminal = g_strcmp0(stage, "error") == 0 ||
                        g_strcmp0(stage, "disconnected") == 0;

    g_free(stage);
    g_free(detail);
    g_free(status_json);

    if (terminal) {
      // Возвращаем G_SOURCE_REMOVE — GLib сам снимает источник; лишь обнуляем id,
      // чтобы stop_polling не сделал двойной g_source_remove по уже мёртвому id.
      self->poll_source_id = 0;
      return G_SOURCE_REMOVE;
    }
  }

  // Traffic.
  gchar* traffic_json = take_string(self, self->ffi.Traffic(self->handle));
  if (traffic_json != NULL) {
    if (self->traffic_listening && self->traffic_channel != NULL) {
      FlValue* map = fl_value_new_map();
      fl_value_set_string_take(
          map, "downBps",
          fl_value_new_int(caramba_json_get_int(traffic_json, "downBps", 0)));
      fl_value_set_string_take(
          map, "upBps",
          fl_value_new_int(caramba_json_get_int(traffic_json, "upBps", 0)));
      fl_value_set_string_take(
          map, "downTotal",
          fl_value_new_int(caramba_json_get_int(traffic_json, "downTotal", 0)));
      fl_value_set_string_take(
          map, "upTotal",
          fl_value_new_int(caramba_json_get_int(traffic_json, "upTotal", 0)));
      fl_event_channel_send(self->traffic_channel, map, NULL, NULL);
      fl_value_unref(map);
    }
    g_free(traffic_json);
  }

  return G_SOURCE_CONTINUE;
}

static void start_polling(CarambaVpnPlugin* self) {
  if (self->poll_source_id == 0) {
    self->poll_source_id = g_timeout_add_seconds(1, poll_tick, self);
  }
}

static void stop_polling(CarambaVpnPlugin* self) {
  if (self->poll_source_id != 0) {
    g_source_remove(self->poll_source_id);
    self->poll_source_id = 0;
  }
}

// --- method channel ----------------------------------------------------------

static void caramba_connect(CarambaVpnPlugin* self, const gchar* server_id) {
  emit_stage(self, "connecting", NULL);
  if (!ensure_core(self)) {
    return;
  }
  // Desktop: mihomo owns the tun device. Pass -1, never establish an fd here.
  // SetTunFd returns NULL on success or an FFI-owned error string; drop it.
  drop_string(self, self->ffi.SetTunFd(self->handle, -1));

  // CarambaUp always returns a non-NULL JSON string: api.UpResult on success or
  // { "error": ... } on failure. Parse the "error" field rather than testing
  // for NULL.
  gchar* up_json =
      take_string(self, self->ffi.Up(self->handle, server_id != NULL ? server_id
                                                                      : ""));
  gchar* up_error =
      up_json != NULL ? caramba_json_get_string(up_json, "error") : NULL;
  if (up_json == NULL || up_error != NULL) {
    emit_stage(self, "error",
               up_error != NULL ? up_error : "tunnel failed to start");
    g_free(up_error);
    g_free(up_json);
    return;
  }
  g_free(up_json);

  start_polling(self);
}

// caramba_connect_raw поднимает туннель из сырой подписки (rawSub-путь). Зеркалит
// caramba_connect, меняя configure->import и serverId->"": ядро парсит raw_config
// формата format в mihomo-конфиг и держит его как импортированный источник, после
// чего Up("") поднимает именно его (у raw-источника узла подписки нет).
static void caramba_connect_raw(CarambaVpnPlugin* self, const gchar* raw_config,
                                const gchar* format, const gchar* server_id) {
  emit_stage(self, "connecting", NULL);
  if (!ensure_core(self)) {
    return;
  }

  // CarambaImportSubscription always returns a non-NULL JSON string: import
  // metadata on success or { "error": ... } on failure. Parse the "error" field
  // rather than testing for NULL, mirroring CarambaUp's convention.
  gchar* import_json = take_string(
      self, self->ffi.ImportSubscription(
                self->handle, raw_config != NULL ? raw_config : "",
                format != NULL ? format : ""));
  gchar* import_error =
      import_json != NULL ? caramba_json_get_string(import_json, "error") : NULL;
  if (import_json == NULL || import_error != NULL) {
    emit_stage(self, "error",
               import_error != NULL ? import_error
                                    : "subscription import failed");
    g_free(import_error);
    g_free(import_json);
    return;
  }
  // Метаданные импорта нам не нужны — освобождаем строку.
  g_free(import_json);

  // Desktop: mihomo owns the tun device. Pass -1, never establish an fd here.
  // SetTunFd returns NULL on success or an FFI-owned error string; drop it.
  drop_string(self, self->ffi.SetTunFd(self->handle, -1));

  // Up поднимает импортированный конфиг. serverId (ABI v2) прикрепляет селектор
  // CARAMBA к конкретному узлу; пусто — автоматический выбор. Как и в
  // caramba_connect, результат всегда non-NULL JSON — проверяем поле "error".
  gchar* up_json = take_string(
      self, self->ffi.Up(self->handle, server_id != NULL ? server_id : ""));
  gchar* up_error =
      up_json != NULL ? caramba_json_get_string(up_json, "error") : NULL;
  if (up_json == NULL || up_error != NULL) {
    emit_stage(self, "error",
               up_error != NULL ? up_error : "tunnel failed to start");
    g_free(up_error);
    g_free(up_json);
    return;
  }
  g_free(up_json);

  start_polling(self);
}

static void caramba_disconnect(CarambaVpnPlugin* self) {
  if (self->handle != 0 && self->ffi.Down != NULL) {
    drop_string(self, self->ffi.Down(self->handle));
  }
  stop_polling(self);
  emit_stage(self, "disconnected", NULL);
}

// method_call_cb dispatches connect / disconnect / status.
static void method_call_cb(FlMethodChannel* channel, FlMethodCall* method_call,
                           gpointer user_data) {
  CarambaVpnPlugin* self = CARAMBA_VPN_PLUGIN(user_data);
  const gchar* method = fl_method_call_get_name(method_call);
  FlValue* args = fl_method_call_get_args(method_call);

  g_autoptr(FlMethodResponse) response = NULL;

  if (g_strcmp0(method, "configure") == 0) {
    const gchar* panel_url = NULL;
    const gchar* subscription_id = NULL;
    const gchar* access_token = NULL;
    if (args != NULL && fl_value_get_type(args) == FL_VALUE_TYPE_MAP) {
      FlValue* v = fl_value_lookup_string(args, "panelUrl");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        panel_url = fl_value_get_string(v);
      }
      // The app's VpnConfig.toArgs() sends subscriptionUuid; the plugin facade
      // sends subscriptionId. Accept either on the same channel.
      v = fl_value_lookup_string(args, "subscriptionUuid");
      if (v == NULL || fl_value_get_type(v) != FL_VALUE_TYPE_STRING) {
        v = fl_value_lookup_string(args, "subscriptionId");
      }
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        subscription_id = fl_value_get_string(v);
      }
      v = fl_value_lookup_string(args, "accessToken");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        access_token = fl_value_get_string(v);
      }
    }
    caramba_configure(self, panel_url, subscription_id, access_token);
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(NULL));
  } else if (g_strcmp0(method, "connect") == 0) {
    const gchar* server_id = NULL;
    if (args != NULL && fl_value_get_type(args) == FL_VALUE_TYPE_MAP) {
      FlValue* v = fl_value_lookup_string(args, "serverId");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        server_id = fl_value_get_string(v);
      }
    }
    caramba_connect(self, server_id);
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(NULL));
  } else if (g_strcmp0(method, "connectRaw") == 0) {
    const gchar* raw_config = NULL;
    const gchar* format = NULL;
    const gchar* server_id = NULL;
    if (args != NULL && fl_value_get_type(args) == FL_VALUE_TYPE_MAP) {
      FlValue* v = fl_value_lookup_string(args, "rawConfig");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        raw_config = fl_value_get_string(v);
      }
      v = fl_value_lookup_string(args, "format");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        format = fl_value_get_string(v);
      }
      // ABI v2: serverId прикрепляет селектор CARAMBA к узлу импорта.
      v = fl_value_lookup_string(args, "serverId");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        server_id = fl_value_get_string(v);
      }
      // label — только для отображения профиля; ядру не передаётся.
    }
    caramba_connect_raw(self, raw_config, format, server_id);
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(NULL));
  } else if (g_strcmp0(method, "importSubscription") == 0) {
    // Generic-режим: разобрать подписку и вернуть метаданные БЕЗ поднятия
    // туннеля. Ядро уже загружено (или создаётся здесь) — тот же handle, что
    // потом поднимет connectRaw, поэтому импорт остаётся активным конфигом.
    const gchar* raw_config = NULL;
    const gchar* format = NULL;
    if (args != NULL && fl_value_get_type(args) == FL_VALUE_TYPE_MAP) {
      FlValue* v = fl_value_lookup_string(args, "rawConfig");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        raw_config = fl_value_get_string(v);
      }
      v = fl_value_lookup_string(args, "format");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        format = fl_value_get_string(v);
      }
    }
    if (!ensure_core(self)) {
      response = FL_METHOD_RESPONSE(fl_method_error_response_new(
          "core_missing", "exarobot core library not found", NULL));
    } else {
      gchar* json = take_string(
          self, self->ffi.ImportSubscription(
                    self->handle, raw_config != NULL ? raw_config : "",
                    format != NULL ? format : ""));
      g_autoptr(FlValue) out = fl_value_new_string(json != NULL ? json : "");
      g_free(json);
      response = FL_METHOD_RESPONSE(fl_method_success_response_new(out));
    }
  } else if (g_strcmp0(method, "probe") == 0) {
    int timeout_ms = 5000;
    if (args != NULL && fl_value_get_type(args) == FL_VALUE_TYPE_MAP) {
      FlValue* v = fl_value_lookup_string(args, "timeoutMs");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_INT) {
        timeout_ms = (int)fl_value_get_int(v);
      }
    }
    if (!ensure_core(self)) {
      response = FL_METHOD_RESPONSE(fl_method_error_response_new(
          "core_missing", "exarobot core library not found", NULL));
    } else if (self->ffi.Probe == NULL) {
      response = FL_METHOD_RESPONSE(fl_method_error_response_new(
          "core_missing", "CarambaProbe is missing (core predates ABI v2)",
          NULL));
    } else {
      gchar* json = take_string(self, self->ffi.Probe(self->handle, timeout_ms));
      g_autoptr(FlValue) out = fl_value_new_string(json != NULL ? json : "");
      g_free(json);
      response = FL_METHOD_RESPONSE(fl_method_success_response_new(out));
    }
  } else if (g_strcmp0(method, "setPolicy") == 0) {
    const gchar* json = NULL;
    if (args != NULL && fl_value_get_type(args) == FL_VALUE_TYPE_MAP) {
      FlValue* v = fl_value_lookup_string(args, "json");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        json = fl_value_get_string(v);
      }
    }
    g_clear_pointer(&self->policy_json, g_free);
    self->policy_json = g_strdup(json != NULL ? json : "");
    if (self->handle != 0 && self->ffi.SetPolicy == NULL) {
      response = FL_METHOD_RESPONSE(fl_method_error_response_new(
          "core_missing", "CarambaSetPolicy is missing (core predates ABI v2)",
          NULL));
    } else {
      apply_policy(self);
      response = FL_METHOD_RESPONSE(fl_method_success_response_new(NULL));
    }
  } else if (g_strcmp0(method, "setTunnelMode") == 0) {
    const gchar* mode = NULL;
    int port = 7890;
    if (args != NULL && fl_value_get_type(args) == FL_VALUE_TYPE_MAP) {
      FlValue* v = fl_value_lookup_string(args, "mode");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_STRING) {
        mode = fl_value_get_string(v);
      }
      v = fl_value_lookup_string(args, "port");
      if (v != NULL && fl_value_get_type(v) == FL_VALUE_TYPE_INT) {
        port = (int)fl_value_get_int(v);
      }
    }
    g_clear_pointer(&self->tunnel_mode, g_free);
    self->tunnel_mode = g_strdup(mode != NULL ? mode : "tun");
    self->mixed_port = port;
    if (self->handle != 0 && self->ffi.SetTunnelMode == NULL) {
      response = FL_METHOD_RESPONSE(fl_method_error_response_new(
          "core_missing",
          "CarambaSetTunnelMode is missing (core predates ABI v2)", NULL));
    } else {
      apply_tunnel_mode(self);
      response = FL_METHOD_RESPONSE(fl_method_success_response_new(NULL));
    }
  } else if (g_strcmp0(method, "disconnect") == 0) {
    caramba_disconnect(self);
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(NULL));
  } else if (g_strcmp0(method, "status") == 0) {
    g_autoptr(FlValue) map = build_status_map(
        self->last_stage != NULL ? self->last_stage : "disconnected",
        self->last_detail, self->last_connected_since_ms);
    response = FL_METHOD_RESPONSE(fl_method_success_response_new(map));
  } else {
    response = FL_METHOD_RESPONSE(fl_method_not_implemented_response_new());
  }

  fl_method_call_respond(method_call, response, NULL);
}

// --- event channels ----------------------------------------------------------

static FlMethodErrorResponse* status_listen_cb(FlEventChannel* channel,
                                               FlValue* args,
                                               gpointer user_data) {
  CarambaVpnPlugin* self = CARAMBA_VPN_PLUGIN(user_data);
  self->status_listening = TRUE;
  // Re-emit the last stage so a fresh subscriber renders immediately — with the
  // real detail + connectedSinceMs, not 0/null.
  g_autoptr(FlValue) map = build_status_map(
      self->last_stage != NULL ? self->last_stage : "disconnected",
      self->last_detail, self->last_connected_since_ms);
  fl_event_channel_send(self->status_channel, map, NULL, NULL);
  return NULL;
}

static FlMethodErrorResponse* status_cancel_cb(FlEventChannel* channel,
                                               FlValue* args,
                                               gpointer user_data) {
  CARAMBA_VPN_PLUGIN(user_data)->status_listening = FALSE;
  return NULL;
}

static FlMethodErrorResponse* traffic_listen_cb(FlEventChannel* channel,
                                                FlValue* args,
                                                gpointer user_data) {
  CARAMBA_VPN_PLUGIN(user_data)->traffic_listening = TRUE;
  return NULL;
}

static FlMethodErrorResponse* traffic_cancel_cb(FlEventChannel* channel,
                                                FlValue* args,
                                                gpointer user_data) {
  CARAMBA_VPN_PLUGIN(user_data)->traffic_listening = FALSE;
  return NULL;
}

// --- lifecycle ---------------------------------------------------------------

static void caramba_vpn_plugin_dispose(GObject* object) {
  CarambaVpnPlugin* self = CARAMBA_VPN_PLUGIN(object);
  stop_polling(self);
  if (self->handle != 0 && self->ffi.Down != NULL) {
    drop_string(self, self->ffi.Down(self->handle));
    self->handle = 0;
  }
  g_clear_object(&self->method_channel);
  g_clear_object(&self->status_channel);
  g_clear_object(&self->traffic_channel);
  g_clear_pointer(&self->last_stage, g_free);
  g_clear_pointer(&self->last_detail, g_free);
  g_clear_pointer(&self->panel_url, g_free);
  g_clear_pointer(&self->subscription_id, g_free);
  g_clear_pointer(&self->access_token, g_free);
  g_clear_pointer(&self->policy_json, g_free);
  g_clear_pointer(&self->tunnel_mode, g_free);
  G_OBJECT_CLASS(caramba_vpn_plugin_parent_class)->dispose(object);
}

static void caramba_vpn_plugin_class_init(CarambaVpnPluginClass* klass) {
  G_OBJECT_CLASS(klass)->dispose = caramba_vpn_plugin_dispose;
}

static void caramba_vpn_plugin_init(CarambaVpnPlugin* self) {
  self->handle = 0;
  self->ffi.module = NULL;
  self->poll_source_id = 0;
  self->status_listening = FALSE;
  self->traffic_listening = FALSE;
  self->last_stage = g_strdup("disconnected");
  self->last_detail = NULL;
  self->last_connected_since_ms = 0;
  self->panel_url = NULL;
  self->subscription_id = NULL;
  self->access_token = NULL;
  self->configured = FALSE;
  self->policy_json = NULL;
  self->tunnel_mode = NULL;
  self->mixed_port = 7890;
}

void caramba_vpn_plugin_register_with_registrar(FlPluginRegistrar* registrar) {
  CarambaVpnPlugin* self = CARAMBA_VPN_PLUGIN(
      g_object_new(caramba_vpn_plugin_get_type(), nullptr));

  FlBinaryMessenger* messenger = fl_plugin_registrar_get_messenger(registrar);
  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();

  // MethodChannel com.caramba/vpn.
  self->method_channel = fl_method_channel_new(
      messenger, CARAMBA_METHOD_CHANNEL, FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(
      self->method_channel, method_call_cb, g_object_ref(self),
      g_object_unref);

  // EventChannel com.caramba/vpn/status.
  self->status_channel = fl_event_channel_new(
      messenger, CARAMBA_STATUS_CHANNEL, FL_METHOD_CODEC(codec));
  fl_event_channel_set_stream_handlers(self->status_channel, status_listen_cb,
                                       status_cancel_cb, g_object_ref(self),
                                       g_object_unref);

  // EventChannel com.caramba/vpn/traffic.
  self->traffic_channel = fl_event_channel_new(
      messenger, CARAMBA_TRAFFIC_CHANNEL, FL_METHOD_CODEC(codec));
  fl_event_channel_set_stream_handlers(self->traffic_channel, traffic_listen_cb,
                                       traffic_cancel_cb, g_object_ref(self),
                                       g_object_unref);

  g_object_unref(self);
}
