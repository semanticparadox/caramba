//! Admin "Payment Pricing & Methods" page.
//!
//! A single self-contained screen that lets the operator:
//!   1. Toggle which payment providers are offered in the Mini App. Toggles write
//!      the same `{provider}_enabled` keys the Mini App reads via
//!      [`provider_enable_setting`], so a change takes effect on the next refresh.
//!   2. Set per-plan-duration price/currency overrides for each provider, stored
//!      through [`CatalogService`] (`plan_duration_provider_prices`). When no
//!      override exists the checkout falls back to the base USD price.
//!
//! The page is rendered as a raw HTML document (vanilla JS + `fetch`) rather than
//! an Askama template so it stays decoupled from the large typed `settings.html`.

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Deserialize;
use serde_json::json;

use crate::AppState;
use crate::api::client::provider_label;
use crate::handlers::admin::auth::is_authenticated;
use crate::services::marketplace_service::provider_enable_setting;

/// GET `{admin}/payment-pricing` — interactive management page.
pub async fn payment_pricing_page(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, Html("Unauthorized".to_string())).into_response();
    }
    Html(render_page(&state.admin_path)).into_response()
}

/// GET `{admin}/payment-pricing/data` — providers + plans + overrides as JSON.
pub async fn payment_pricing_data(
    State(state): State<AppState>,
    jar: CookieJar,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, Json(json!({"ok": false}))).into_response();
    }

    // Registered providers (= credentials present) with their current on/off state.
    let mut providers = Vec::new();
    for name in state.marketplace_service.provider_names() {
        if name == "balance" {
            continue;
        }
        let (key, default_on) = provider_enable_setting(&name);
        let default = if default_on { "true" } else { "false" };
        let enabled = state.settings.get_or_default(&key, default).await == "true";
        providers.push(json!({
            "id": name,
            "label": provider_label(&name),
            "enabled": enabled,
        }));
    }
    providers.sort_by(|a, b| a["label"].as_str().cmp(&b["label"].as_str()));

    // Active plans with their durations and any existing per-provider overrides.
    let mut plans_json = Vec::new();
    if let Ok(plans) = state.catalog_service.get_plans_admin().await {
        for plan in plans {
            let mut durations = Vec::new();
            for d in &plan.durations {
                let overrides = state.catalog_service.list_duration_overrides(d.id).await;
                let ov_map: serde_json::Map<String, serde_json::Value> = overrides
                    .into_iter()
                    .map(|(p, (amount, currency))| {
                        (p, json!({ "amount": amount, "currency": currency }))
                    })
                    .collect();
                durations.push(json!({
                    "id": d.id,
                    "days": d.duration_days,
                    "base_price": d.price,
                    "overrides": ov_map,
                }));
            }
            plans_json.push(json!({
                "id": plan.id,
                "name": plan.name,
                "durations": durations,
            }));
        }
    }

    Json(json!({
        "ok": true,
        "providers": providers,
        "plans": plans_json,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct ToggleBody {
    pub provider: String,
    pub enabled: bool,
}

/// POST `{admin}/payment-pricing/toggle` — enable/disable a provider in the Mini App.
pub async fn payment_pricing_toggle(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<ToggleBody>,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, Json(json!({"ok": false}))).into_response();
    }
    let (key, _default) = provider_enable_setting(&body.provider);
    let value = if body.enabled { "true" } else { "false" };
    match state.settings.set(&key, value).await {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct OverrideBody {
    pub duration_id: i64,
    pub provider: String,
    /// Amount in minor units (cents/kopecks). `<= 0` clears the override.
    pub amount: i64,
    pub currency: String,
}

/// POST `{admin}/payment-pricing/override` — upsert or clear a per-duration override.
pub async fn payment_pricing_override(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<OverrideBody>,
) -> impl IntoResponse {
    if !is_authenticated(&state, &jar).await {
        return (StatusCode::UNAUTHORIZED, Json(json!({"ok": false}))).into_response();
    }
    let currency = body.currency.trim().to_uppercase();
    let result = if body.amount <= 0 || currency.is_empty() {
        state
            .catalog_service
            .delete_duration_override(body.duration_id, &body.provider)
            .await
    } else {
        state
            .catalog_service
            .upsert_duration_override(body.duration_id, &body.provider, body.amount, &currency)
            .await
    };
    match result {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Builds the standalone HTML page. `__ADMIN_PATH__` is substituted (rather than
/// using `format!`) so the embedded CSS/JS braces don't need escaping.
fn render_page(admin_path: &str) -> String {
    PAGE_TEMPLATE.replace("__ADMIN_PATH__", admin_path)
}

const PAGE_TEMPLATE: &str = r###"<!DOCTYPE html>
<html lang="ru">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Способы оплаты и цены</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body { margin: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
         background: #0b0f14; color: #e6edf3; }
  a { color: #58a6ff; }
  .wrap { max-width: 1080px; margin: 0 auto; padding: 24px 16px 80px; }
  header.top { display: flex; align-items: center; justify-content: space-between; margin-bottom: 20px; }
  h1 { font-size: 22px; margin: 0; }
  h2 { font-size: 16px; margin: 28px 0 12px; color: #9fb0c0; text-transform: uppercase; letter-spacing: .04em; }
  .card { background: #11161d; border: 1px solid #1f2a36; border-radius: 12px; padding: 16px; }
  .hint { color: #8a97a6; font-size: 13px; line-height: 1.5; }
  .providers { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 10px; }
  .prov { display: flex; align-items: center; justify-content: space-between; gap: 10px;
          background: #0e141b; border: 1px solid #1f2a36; border-radius: 10px; padding: 12px 14px; }
  .prov .lbl { font-size: 14px; }
  .switch { position: relative; width: 44px; height: 24px; flex: none; }
  .switch input { opacity: 0; width: 0; height: 0; }
  .slider { position: absolute; inset: 0; background: #30363d; border-radius: 24px; cursor: pointer; transition: .2s; }
  .slider:before { content: ""; position: absolute; height: 18px; width: 18px; left: 3px; top: 3px;
                   background: #fff; border-radius: 50%; transition: .2s; }
  .switch input:checked + .slider { background: #238636; }
  .switch input:checked + .slider:before { transform: translateX(20px); }
  select, input[type=text], input[type=number] {
    background: #0e141b; border: 1px solid #2a3744; color: #e6edf3; border-radius: 8px;
    padding: 8px 10px; font-size: 14px; }
  select { min-width: 260px; }
  table { width: 100%; border-collapse: collapse; margin-top: 8px; }
  th, td { text-align: left; padding: 8px 10px; border-bottom: 1px solid #1b242e; font-size: 14px; vertical-align: middle; }
  th { color: #8a97a6; font-weight: 600; }
  .dur-block { margin-top: 16px; }
  .dur-head { font-size: 15px; font-weight: 600; margin-bottom: 4px; }
  .dur-sub { font-size: 12px; color: #8a97a6; }
  input.amt { width: 110px; }
  input.cur { width: 90px; text-transform: uppercase; }
  button { background: #238636; border: none; color: #fff; border-radius: 8px; padding: 7px 12px;
           font-size: 13px; cursor: pointer; }
  button.ghost { background: #21262d; color: #c9d1d9; }
  button:disabled { opacity: .5; cursor: default; }
  .row-actions { display: flex; gap: 6px; }
  .ov-on td:first-child { border-left: 3px solid #58a6ff; }
  .toast { position: fixed; bottom: 20px; left: 50%; transform: translateX(-50%);
           background: #238636; color: #fff; padding: 10px 18px; border-radius: 8px; opacity: 0;
           transition: opacity .25s; pointer-events: none; font-size: 14px; }
  .toast.err { background: #da3633; }
  .toast.show { opacity: 1; }
  .empty { color: #8a97a6; font-size: 14px; padding: 12px 0; }
  code { background: #0e141b; padding: 1px 6px; border-radius: 5px; }
</style>
</head>
<body>
<div class="wrap">
  <header class="top">
    <h1>Способы оплаты и цены</h1>
    <a href="__ADMIN_PATH__/settings">← Настройки</a>
  </header>

  <div class="card">
    <p class="hint">
      Включайте здесь методы оплаты — они появятся в Mini App для пользователей.
      Реквизиты (API-ключи) задаются в разделе <a href="__ADMIN_PATH__/settings">Настройки → Платежи</a>;
      в этом списке показаны только методы с заполненными ключами.
    </p>
  </div>

  <h2>Методы оплаты</h2>
  <div id="providers" class="providers"><div class="empty">Загрузка…</div></div>

  <h2>Цены по способам оплаты</h2>
  <div class="card">
    <p class="hint">
      Базовая цена тарифа указана в <b>USD</b>. Для любого метода можно задать свою цену и валюту
      (например, <code>RUB</code> для Tribute/WATA, <code>USDT</code> для крипто-методов).
      Оставьте сумму пустой и нажмите «Сбросить», чтобы вернуть базовую цену в USD.
      Сумма указывается в основной единице валюты (например, <code>499</code> = 499 ₽).
    </p>
    <div style="margin-top:12px;">
      <label class="hint">Тариф: </label>
      <select id="planSelect"></select>
    </div>
    <div id="durations"></div>
  </div>
</div>

<div id="toast" class="toast"></div>

<script>
const ADMIN = "__ADMIN_PATH__";
let DATA = { providers: [], plans: [] };

function toast(msg, isErr) {
  const t = document.getElementById('toast');
  t.textContent = msg;
  t.className = 'toast show' + (isErr ? ' err' : '');
  setTimeout(() => { t.className = 'toast' + (isErr ? ' err' : ''); }, 2200);
}

async function api(path, body) {
  const res = await fetch(ADMIN + path, {
    method: body ? 'POST' : 'GET',
    headers: body ? { 'Content-Type': 'application/json' } : {},
    body: body ? JSON.stringify(body) : undefined,
    credentials: 'same-origin',
  });
  return res.json();
}

function enabledProviders() {
  return DATA.providers.filter(p => p.enabled);
}

function renderProviders() {
  const box = document.getElementById('providers');
  if (!DATA.providers.length) {
    box.innerHTML = '<div class="empty">Нет настроенных методов. Добавьте API-ключи в Настройках.</div>';
    return;
  }
  box.innerHTML = '';
  DATA.providers.forEach(p => {
    const el = document.createElement('div');
    el.className = 'prov';
    el.innerHTML = '<span class="lbl">' + p.label + '</span>'
      + '<label class="switch"><input type="checkbox"' + (p.enabled ? ' checked' : '')
      + '><span class="slider"></span></label>';
    const cb = el.querySelector('input');
    cb.addEventListener('change', async () => {
      cb.disabled = true;
      const r = await api('/payment-pricing/toggle', { provider: p.id, enabled: cb.checked });
      cb.disabled = false;
      if (r && r.ok) {
        p.enabled = cb.checked;
        toast(p.label + (cb.checked ? ' включён' : ' выключен'));
        renderDurations();
      } else {
        cb.checked = !cb.checked;
        toast('Не удалось сохранить', true);
      }
    });
    box.appendChild(el);
  });
}

function renderPlanSelect() {
  const sel = document.getElementById('planSelect');
  sel.innerHTML = '';
  if (!DATA.plans.length) {
    const o = document.createElement('option');
    o.textContent = 'Нет тарифов';
    sel.appendChild(o);
    return;
  }
  DATA.plans.forEach((pl, i) => {
    const o = document.createElement('option');
    o.value = String(i);
    o.textContent = pl.name;
    sel.appendChild(o);
  });
  sel.onchange = renderDurations;
  renderDurations();
}

function renderDurations() {
  const box = document.getElementById('durations');
  box.innerHTML = '';
  const sel = document.getElementById('planSelect');
  const plan = DATA.plans[parseInt(sel.value || '0', 10)];
  if (!plan) { box.innerHTML = '<div class="empty">Нет тарифов.</div>'; return; }
  const provs = enabledProviders();
  if (!provs.length) {
    box.innerHTML = '<div class="empty">Сначала включите хотя бы один метод оплаты выше.</div>';
    return;
  }
  if (!plan.durations.length) {
    box.innerHTML = '<div class="empty">У тарифа нет длительностей.</div>';
    return;
  }
  plan.durations.forEach(d => {
    const block = document.createElement('div');
    block.className = 'dur-block';
    const baseUsd = (d.base_price / 100).toFixed(2);
    block.innerHTML = '<div class="dur-head">' + d.days + ' дней</div>'
      + '<div class="dur-sub">Базовая цена: ' + baseUsd + ' USD</div>'
      + '<table><thead><tr><th>Метод</th><th>Сумма</th><th>Валюта</th><th></th></tr></thead>'
      + '<tbody></tbody></table>';
    const tbody = block.querySelector('tbody');
    provs.forEach(p => {
      const ov = (d.overrides && d.overrides[p.id]) || null;
      const tr = document.createElement('tr');
      if (ov) tr.className = 'ov-on';
      const amtVal = ov ? (ov.amount / 100) : '';
      const curVal = ov ? ov.currency : '';
      tr.innerHTML = '<td>' + p.label + '</td>'
        + '<td><input type="number" step="0.01" min="0" class="amt" value="' + amtVal
        + '" placeholder="' + baseUsd + '"></td>'
        + '<td><input type="text" class="cur" value="' + curVal + '" placeholder="USD"></td>'
        + '<td class="row-actions"><button class="save">Сохранить</button>'
        + '<button class="ghost clear">Сбросить</button></td>';
      const amt = tr.querySelector('.amt');
      const cur = tr.querySelector('.cur');
      tr.querySelector('.save').addEventListener('click', async () => {
        const a = parseFloat(amt.value);
        const c = (cur.value || '').trim().toUpperCase();
        if (!a || a <= 0 || !c) { toast('Укажите сумму и валюту', true); return; }
        const r = await api('/payment-pricing/override', {
          duration_id: d.id, provider: p.id, amount: Math.round(a * 100), currency: c });
        if (r && r.ok) {
          d.overrides = d.overrides || {};
          d.overrides[p.id] = { amount: Math.round(a * 100), currency: c };
          tr.className = 'ov-on';
          toast('Сохранено: ' + p.label + ' → ' + a + ' ' + c);
        } else { toast('Ошибка сохранения', true); }
      });
      tr.querySelector('.clear').addEventListener('click', async () => {
        const r = await api('/payment-pricing/override', {
          duration_id: d.id, provider: p.id, amount: 0, currency: '' });
        if (r && r.ok) {
          if (d.overrides) delete d.overrides[p.id];
          amt.value = ''; cur.value = ''; tr.className = '';
          toast('Сброшено: ' + p.label);
        } else { toast('Ошибка', true); }
      });
      tbody.appendChild(tr);
    });
    box.appendChild(block);
  });
}

async function init() {
  const r = await api('/payment-pricing/data');
  if (!r || !r.ok) { toast('Не удалось загрузить данные', true); return; }
  DATA = r;
  renderProviders();
  renderPlanSelect();
}
init();
</script>
</body>
</html>
"###;
