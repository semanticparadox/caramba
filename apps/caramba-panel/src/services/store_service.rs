use crate::services::activity_service::ActivityService;
use crate::services::referral_service::ReferralService;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use caramba_db::models::store::{CartItem, GiftCode, PlanDuration, Subscription, User};

use caramba_db::repositories::api_key_repo::ApiKeyRepository;
use caramba_db::repositories::node_repo::NodeRepository;
use caramba_db::repositories::subscription_repo::{
    ACTIVE_SUBSCRIPTIONS_FOR_UPDATE_SQL, STATUS_SUPERSEDED, SubscriptionRepository,
};
use caramba_db::repositories::user_repo::UserRepository;

/// Результат покупки тарифа: подписка создана активной, либо создан подарочный код.
#[derive(Debug)]
pub enum PurchaseResult {
    Subscription(Subscription),
    GiftCode(String),
}

/// Какую дату истечения получит подписка.
#[derive(Debug, Clone, Copy)]
pub enum SubscriptionExpiry {
    /// Точная дата — подарок админа, промо-код, gift-код: срок отсчитывается от
    /// момента выдачи и не зависит от того, что было раньше.
    Exactly(DateTime<Utc>),
    /// Продление на N дней: от текущей даты истечения, если она в будущем,
    /// иначе от «сейчас». Оплаченные дни не сгорают, но и не начисляются задним
    /// числом за месяцы простоя.
    AddDays(i64),
}

impl SubscriptionExpiry {
    fn resolve(self, current: Option<DateTime<Utc>>, now: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            SubscriptionExpiry::Exactly(dt) => dt,
            SubscriptionExpiry::AddDays(days) => {
                let base = match current {
                    Some(current) if current > now => current,
                    _ => now,
                };
                base + Duration::days(days)
            }
        }
    }
}

/// Выдача подписки: всё, что нужно знать общему методу активации.
#[derive(Debug, Clone)]
pub struct SubscriptionGrant<'a> {
    pub user_id: i64,
    pub plan_id: i64,
    pub expiry: SubscriptionExpiry,
    /// `'active'` или `'pending'` (инстанс с ручным одобрением, см.
    /// `license::initial_subscription_status`). Ничего третьего сюда не
    /// передают: «выдать подписку» означает либо дать доступ, либо поставить в
    /// очередь на одобрение.
    pub status: &'a str,
    pub note: Option<&'a str>,
    pub is_trial: bool,
    /// Нода, если путь выдачи её выбирает (подарок админа). `None` — подписку
    /// раскатит оркестрация по всем нодам плана.
    pub node_id: Option<i64>,
}

/// ЕДИНСТВЕННЫЙ путь, которым подписка становится активной.
///
/// Инвариант, который он держит: у пользователя ровно одна строка
/// `status = 'active'`. До этого каждый путь выдачи вставлял новую строку и не
/// трогал старую («Never touches existing subs» — так и было написано в
/// промо-сервисе), из-за чего у оплатившего человека рядом с купленным планом
/// оставалась вечная бесплатная подписка, и разные части системы обслуживали
/// разные строки одного аккаунта.
///
/// Правила:
///   * тот же план уже активен — продлеваем ЕГО (новая строка означала бы
///     потерю vless_uuid, а значит и всех выданных ссылок);
///   * другой план — старые активные строки переводим в `'superseded'`, лизы
///     устройств переносим на новую (иначе смена тарифа молча отвязывала бы все
///     устройства: их отпечаток завязан на subscription_id);
///   * статус `'pending'` (ждёт одобрения админа) ничего не вытесняет — доступ
///     ещё не выдан; вытеснение произойдёт в момент одобрения.
///
/// Транзакция — вызывающей стороны: списание баланса и выдача подписки обязаны
/// быть атомарны.
pub async fn activate_or_replace_subscription_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    grant: SubscriptionGrant<'_>,
) -> Result<Subscription> {
    let now = Utc::now();

    let active = sqlx::query_as::<_, Subscription>(ACTIVE_SUBSCRIPTIONS_FOR_UPDATE_SQL)
        .bind(grant.user_id)
        .fetch_all(&mut **tx)
        .await
        .context("Failed to lock active subscriptions")?;

    let same_plan = active.iter().find(|s| s.plan_id == grant.plan_id).cloned();
    let expires_at = grant
        .expiry
        .resolve(same_plan.as_ref().map(|s| s.expires_at), now);

    // Подписка, которая ещё не активна, никого не вытесняет.
    if grant.status != "active" {
        return insert_subscription_tx(tx, &grant, expires_at).await;
    }

    let sub = match same_plan {
        Some(existing) => {
            // used_traffic = 0: продление открывает новый расчётный период,
            // иначе упёршийся в квоту пользователь сразу снова блокируется.
            // vless_uuid лечится здесь же: строка без него существует в базе,
            // видна в кабинете и не попадает ни в один конфиг ноды.
            sqlx::query_as::<_, Subscription>(
                "UPDATE subscriptions \
                 SET expires_at = $1, status = 'active', used_traffic = 0, \
                     activated_at = COALESCE(activated_at, CURRENT_TIMESTAMP), \
                     note = COALESCE($2, note), \
                     node_id = COALESCE($3, node_id), \
                     vless_uuid = COALESCE(NULLIF(vless_uuid, ''), gen_random_uuid()::TEXT) \
                 WHERE id = $4 \
                 RETURNING *",
            )
            .bind(expires_at)
            .bind(grant.note)
            .bind(grant.node_id)
            .bind(existing.id)
            .fetch_one(&mut **tx)
            .await
            .context("Failed to extend the active subscription")?
        }
        None => insert_subscription_tx(tx, &grant, expires_at).await?,
    };

    let losers: Vec<i64> = active
        .iter()
        .map(|s| s.id)
        .filter(|id| *id != sub.id)
        .collect();
    supersede_subscriptions_tx(tx, &losers, sub.id).await?;

    Ok(sub)
}

/// Версия на собственной транзакции — для путей, которым нечего разделять с
/// вызывающим кодом.
pub async fn activate_or_replace_subscription(
    pool: &PgPool,
    grant: SubscriptionGrant<'_>,
) -> Result<Subscription> {
    let mut tx = pool.begin().await?;
    let sub = activate_or_replace_subscription_tx(&mut tx, grant).await?;
    tx.commit().await?;
    Ok(sub)
}

/// Вставка новой строки подписки — один INSERT на всю панель.
///
/// ИНВАРИАНТ: `vless_uuid` и `subscription_uuid` обязательны. Генерация
/// конфигов нод выбирает `s.vless_uuid` и молча пропускает строки с NULL, а
/// вылечить их потом нечему.
async fn insert_subscription_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    grant: &SubscriptionGrant<'_>,
    expires_at: DateTime<Utc>,
) -> Result<Subscription> {
    sqlx::query_as::<_, Subscription>(
        "INSERT INTO subscriptions \
         (user_id, plan_id, node_id, vless_uuid, subscription_uuid, status, expires_at, \
          note, is_trial, used_traffic, created_at, activated_at) \
         VALUES ($1, $2, $3, gen_random_uuid()::TEXT, gen_random_uuid()::TEXT, $4, $5, \
                 $6, $7, 0, CURRENT_TIMESTAMP, \
                 CASE WHEN $4 = 'active' THEN CURRENT_TIMESTAMP ELSE NULL END) \
         RETURNING *",
    )
    .bind(grant.user_id)
    .bind(grant.plan_id)
    .bind(grant.node_id)
    .bind(grant.status)
    .bind(expires_at)
    .bind(grant.note)
    .bind(grant.is_trial)
    .fetch_one(&mut **tx)
    .await
    .context("Failed to create subscription")
}

/// Переводит перечисленные подписки в `'superseded'` и переносит их лизы
/// устройств на оставшуюся.
///
/// Перенос лиз — половина смысла этой операции: отпечаток устройства считается
/// от `subscription_id`, поэтому без переноса смена тарифа обнуляла бы все
/// привязки, а пользователь снова упирался бы в лимит устройств на своих же
/// телефонах. Лизы, для которых на оставшейся подписке уже есть такой же
/// отпечаток, переехать не могут (UNIQUE) и удаляются как точные дубли.
///
/// Лимит устройств нового плана здесь намеренно не применяется: лимит — это
/// гейт подключения, а не повод молча отвязать уже привязанное устройство.
///
/// Идемпотентно: строки, которые уже вытеснил триггер
/// `trg_subscriptions_single_active`, просто не попадают под условие статуса.
async fn supersede_subscriptions_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    loser_ids: &[i64],
    keeper_id: i64,
) -> Result<()> {
    if loser_ids.is_empty() {
        return Ok(());
    }

    // DISTINCT ON: на оставшуюся подписку обязана приехать ровно одна лиза на
    // отпечаток (UNIQUE(subscription_id, device_fingerprint)) — берём самую
    // свежую. Одного NOT EXISTS мало: он не видит строк, которые этот же UPDATE
    // переносит прямо сейчас, и две вытесняемые подписки с общим отпечатком
    // роняли бы всю выдачу подписки об уникальный индекс.
    sqlx::query(
        "UPDATE subscription_device_leases AS l \
         SET subscription_id = $1 \
         WHERE l.id IN ( \
             SELECT DISTINCT ON (l2.device_fingerprint) l2.id \
             FROM subscription_device_leases l2 \
             WHERE l2.subscription_id = ANY($2) \
               AND NOT EXISTS ( \
                   SELECT 1 FROM subscription_device_leases t \
                   WHERE t.subscription_id = $1 \
                     AND t.device_fingerprint = l2.device_fingerprint) \
             ORDER BY l2.device_fingerprint, l2.last_seen_at DESC, l2.id DESC)",
    )
    .bind(keeper_id)
    .bind(loser_ids)
    .execute(&mut **tx)
    .await
    .context("Failed to move device leases to the surviving subscription")?;

    sqlx::query("DELETE FROM subscription_device_leases WHERE subscription_id = ANY($1)")
        .bind(loser_ids)
        .execute(&mut **tx)
        .await
        .context("Failed to drop duplicate device leases")?;

    let superseded = sqlx::query(
        "UPDATE subscriptions SET status = $1 WHERE id = ANY($2) AND status = 'active'",
    )
    .bind(STATUS_SUPERSEDED)
    .bind(loser_ids)
    .execute(&mut **tx)
    .await
    .context("Failed to supersede the previous subscriptions")?
    .rows_affected();

    if superseded > 0 {
        tracing::info!(
            keeper_id,
            superseded,
            "подписка заменена: прежние активные строки вытеснены"
        );
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct StoreService {
    pool: PgPool,
    user_repo: UserRepository,
    pub sub_repo: SubscriptionRepository,
    pub node_repo: NodeRepository,
    pub api_key_repo: ApiKeyRepository,
    // Use RwLock for interior mutability to break circular dependency with OrchestrationService
    pub orchestration_service: std::sync::Arc<
        std::sync::RwLock<
            Option<std::sync::Arc<crate::services::orchestration_service::OrchestrationService>>,
        >,
    >,
}

impl StoreService {
    pub fn new(pool: PgPool) -> Self {
        let user_repo = UserRepository::new(pool.clone());
        let sub_repo = SubscriptionRepository::new(pool.clone());
        let node_repo = NodeRepository::new(pool.clone());
        let api_key_repo = ApiKeyRepository::new(pool.clone());
        Self {
            pool,
            user_repo,
            sub_repo,
            node_repo,
            api_key_repo,
            orchestration_service: std::sync::Arc::new(std::sync::RwLock::new(None)),
        }
    }

    pub fn set_orchestration_service(
        &self,
        svc: std::sync::Arc<crate::services::orchestration_service::OrchestrationService>,
    ) {
        if let Ok(mut lock) = self.orchestration_service.write() {
            *lock = Some(svc);
        }
    }

    pub fn get_pool(&self) -> PgPool {
        self.pool.clone()
    }

    pub async fn get_products_by_category(
        &self,
        category_id: i64,
    ) -> Result<Vec<caramba_db::models::store::Product>> {
        sqlx::query_as::<_, caramba_db::models::store::Product>(
            "SELECT id, category_id, name, description, price, product_type, content, is_active, created_at FROM products WHERE category_id = $1 AND is_active = TRUE"
        )
        .bind(category_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch products")
    }

    pub async fn get_active_nodes(&self) -> Result<Vec<caramba_db::models::node::Node>> {
        self.node_repo.get_active_nodes().await
    }

    pub async fn get_api_keys(&self) -> Result<Vec<caramba_db::models::api_key::ApiKey>> {
        self.api_key_repo.get_all().await
    }

    pub async fn create_api_key(
        &self,
        name: &str,
        key: &str,
        max_uses: Option<i64>,
    ) -> Result<caramba_db::models::api_key::ApiKey> {
        self.api_key_repo.create(name, key, max_uses).await
    }

    pub async fn delete_api_key(&self, id: i64) -> Result<()> {
        self.api_key_repo.delete(id).await
    }

    pub async fn get_active_subs_by_plans(
        &self,
        plan_ids: &[i64],
    ) -> Result<Vec<(i64, Option<String>, i64, Option<String>)>> {
        self.sub_repo.get_active_subs_by_plans(plan_ids).await
    }

    pub async fn get_subscription_by_uuid(&self, uuid: &str) -> Result<Option<Subscription>> {
        self.sub_repo.get_by_uuid(uuid).await
    }

    pub async fn update_subscription_status(&self, sub_id: i64, status: &str) -> Result<()> {
        self.sub_repo.update_status(sub_id, status).await
    }

    pub async fn reset_warning_count(&self, user_id: i64) -> Result<()> {
        self.user_repo.update_warning_count(user_id, 0).await
    }

    pub async fn get_user_nodes(
        &self,
        user_id: i64,
    ) -> Result<Vec<caramba_db::models::node::Node>> {
        let plan_id = self.sub_repo.get_active_plan_id_by_user(user_id).await?;
        match plan_id {
            Some(id) => self.node_repo.get_nodes_for_plan(id).await,
            None => Ok(vec![]),
        }
    }

    pub async fn get_user_by_tg_id(&self, tg_id: i64) -> Result<Option<User>> {
        self.user_repo.get_by_tg_id(tg_id).await
    }

    /// Поиск пользователя по внутреннему id. Нужен standalone-приложению, где
    /// JWT несёт `user_id` (а не tg_id) — например, при создании чек-аута покупки.
    pub async fn get_user_by_id(&self, id: i64) -> Result<Option<User>> {
        self.user_repo.get_by_id(id).await
    }

    pub async fn get_user_by_referral_code(&self, code: &str) -> Result<Option<User>> {
        self.user_repo.get_by_referral_code(code).await
    }

    pub async fn resolve_referrer_id(&self, code: &str) -> Result<Option<i64>> {
        if let Ok(tg_id) = code.parse::<i64>()
            && let Some(user) = self.get_user_by_tg_id(tg_id).await?
        {
            return Ok(Some(user.id));
        }

        if let Some(user) = self.get_user_by_referral_code(code).await? {
            return Ok(Some(user.id));
        }

        // Partner per-source code -> owning partner user (same attribution path
        // as a plain referral code). Lets bot /start deep links credit partners.
        if let Some(partner_id) = sqlx::query_scalar::<_, i64>(
            "SELECT partner_user_id FROM partner_codes WHERE code = $1",
        )
        .bind(code.trim())
        .fetch_optional(&self.pool)
        .await?
        {
            return Ok(Some(partner_id));
        }

        Ok(None)
    }

    /// Resolves a partner_codes.id for a raw signup code, or None when the code
    /// is not a partner code. Used to stamp users.signup_partner_code_id so
    /// per-code stats (signups/conversions) are derivable. Also bumps the code's
    /// best-effort `clicks` counter (one deep-link signup hit), so call it at
    /// most once per signup attribution.
    pub async fn resolve_partner_code_id(&self, code: &str) -> Result<Option<i64>> {
        let id: Option<i64> = sqlx::query_scalar(
            "UPDATE partner_codes SET clicks = clicks + 1 WHERE code = $1 RETURNING id",
        )
        .bind(code.trim())
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    /// Stamps the partner code a user signed up through, once. Never overwrites
    /// an existing value (attribution is immutable, like referrer_id).
    pub async fn set_signup_partner_code(&self, user_id: i64, partner_code_id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE users SET signup_partner_code_id = $1 \
             WHERE id = $2 AND signup_partner_code_id IS NULL",
        )
        .bind(partner_code_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_user(
        &self,
        tg_id: i64,
        username: Option<&str>,
        full_name: Option<&str>,
        referrer_id: Option<i64>,
    ) -> Result<User> {
        let (user, _was_new) = self
            .upsert_user_with_new_flag(tg_id, username, full_name, referrer_id)
            .await?;
        Ok(user)
    }

    /// Like `upsert_user` but returns whether the user row was newly created.
    /// Use this when the caller wants to fire welcome-notification / first-touch
    /// side effects exactly once per real signup.
    pub async fn upsert_user_with_new_flag(
        &self,
        tg_id: i64,
        username: Option<&str>,
        full_name: Option<&str>,
        referrer_id: Option<i64>,
    ) -> Result<(User, bool)> {
        let existing = self.user_repo.get_by_tg_id(tg_id).await?;

        // License gate (P4, contract E): block creating a NEW user beyond
        // max_users. Existing users always pass; max_users == 0 = unlimited (Pro).
        if existing.is_none() {
            let limits = crate::license::effective_limits_from_pool(&self.pool).await;
            if limits.max_users != 0 {
                let current: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
                    .fetch_one(&self.pool)
                    .await
                    .unwrap_or(0);
                crate::license::check_can_add_user(&limits, current)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
        }

        let user = self
            .user_repo
            .upsert(tg_id, username, full_name, referrer_id)
            .await?;

        let was_new = existing.is_none();
        if was_new {
            let _ =
                crate::services::analytics_service::AnalyticsService::track_new_user(&self.pool)
                    .await;

            // U26: auto-trigger the referral SIGNUP bonus exactly once, on the
            // first creation of a user that has a referrer. Previously this only
            // fired via the external /api/v2/bot/referral/signup-bonus endpoint,
            // so signup bonuses never happened in practice for bot /start or any
            // other path that goes through this central creation function.
            //
            // We rely on the persisted `user.referrer_id` (set by the upsert) and
            // fall back to the caller-supplied `referrer_id` for safety. The
            // crediting itself is idempotent (referral_bonuses table guards
            // duplicates), so this is also safe if anything races. We only credit
            // on genuine first creation — repeat /start (upsert) hits the
            // `existing.is_some()` branch and skips this entirely, so no
            // double-crediting.
            if let Some(r_id) = user.referrer_id.or(referrer_id) {
                // Never credit a self-referral.
                if r_id != user.id
                    && let Err(e) =
                        ReferralService::apply_signup_bonus(&self.pool, r_id, user.id).await
                {
                    tracing::warn!(
                        referrer_id = r_id,
                        referred_user_id = user.id,
                        error = %e,
                        "failed to apply referral signup bonus on user creation"
                    );
                }
            }
        }
        let _ = crate::services::analytics_service::AnalyticsService::track_active_user(
            &self.pool, user.id,
        )
        .await;

        Ok((user, was_new))
    }

    pub async fn create_family_invite(
        &self,
        parent_id: i64,
        max_uses: i32,
        duration_days: i32,
    ) -> Result<caramba_db::models::store::FamilyInvite> {
        let random_part = Uuid::new_v4()
            .to_string()
            .replace("-", "")
            .chars()
            .take(6)
            .collect::<String>()
            .to_uppercase();
        let code = format!("FAMILY-{}", random_part);
        let expires_at = Utc::now() + Duration::days(duration_days as i64);

        let invite = sqlx::query_as::<_, caramba_db::models::store::FamilyInvite>(
            "INSERT INTO family_invites (code, parent_id, max_uses, expires_at) VALUES ($1, $2, $3, $4) RETURNING *"
        )
        .bind(code)
        .bind(parent_id)
        .bind(max_uses)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await
        .context("Failed to create family invite")?;

        Ok(invite)
    }

    pub async fn get_valid_invite(
        &self,
        code: &str,
    ) -> Result<Option<caramba_db::models::store::FamilyInvite>> {
        let invite = sqlx::query_as::<_, caramba_db::models::store::FamilyInvite>(
            "SELECT * FROM family_invites WHERE code = $1 AND expires_at > CURRENT_TIMESTAMP AND used_count < max_uses"
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;

        Ok(invite)
    }

    pub async fn redeem_family_invite(&self, user_id: i64, code: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        let invite = sqlx::query_as::<_, caramba_db::models::store::FamilyInvite>(
            "SELECT * FROM family_invites WHERE code = $1 AND expires_at > CURRENT_TIMESTAMP AND used_count < max_uses FOR UPDATE"
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let invite = match invite {
            Some(i) => i,
            None => return Err(anyhow::anyhow!("Invalid or expired invite code")),
        };

        if invite.parent_id == user_id {
            return Err(anyhow::anyhow!("You cannot invite yourself"));
        }

        let current_user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        if let Some(pid) = current_user.parent_id {
            if pid == invite.parent_id {
                return Err(anyhow::anyhow!("You are already in this family"));
            }
            return Err(anyhow::anyhow!(
                "You are already a member of another family"
            ));
        }

        sqlx::query("UPDATE users SET parent_id = $1 WHERE id = $2")
            .bind(invite.parent_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE family_invites SET used_count = used_count + 1 WHERE id = $1")
            .bind(invite.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        // Best-effort — sync failure should not roll back the invite acceptance.
        if let Err(e) = self.sync_family_subscriptions(invite.parent_id).await {
            tracing::warn!(parent_id = invite.parent_id, error = %e, "family sync after invite failed (children may be stale)");
        }
        Ok(())
    }

    // ============================================================
    // ENROLLMENT CODES (standalone app — Caramba Connect)
    // ============================================================

    /// Чистая READ-ONLY валидация кода вовлечения. НЕ списывает использование и
    /// НЕ берёт row lock — нужна публичному эндпоинту GET /enroll/{code}, который
    /// обязан быть идемпотентным чтением. Возвращает Some(code) если код существует
    /// и валиден (не истёк, использования не исчерпаны), иначе None.
    ///
    /// Предикат валидности учитывает нуллабельный expires_at:
    /// `(expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP) AND used_count < max_uses`.
    pub async fn validate_enrollment_code(
        &self,
        code: &str,
    ) -> Result<Option<caramba_db::models::store::EnrollmentCode>> {
        let row = sqlx::query_as::<_, caramba_db::models::store::EnrollmentCode>(
            "SELECT * FROM enrollment_codes \
             WHERE code = $1 \
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP) \
               AND used_count < max_uses",
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Списывает (consume) код вовлечения для ТОЛЬКО ЧТО созданного пользователя
    /// и, если настроено, выдаёт одноразовый онбординг-трафик. Вся работа — в
    /// ОДНОЙ транзакции, поэтому списание used_count служит якорем идемпотентности:
    /// двойной сабмит не может ни дважды декрементировать used_traffic, ни
    /// превысить max_uses.
    ///
    /// Шаги внутри транзакции:
    ///   1. SELECT ... FOR UPDATE по предикату валидности — лочим строку кода.
    ///   2. Условный UPDATE used_count = used_count + 1 WHERE used_count < max_uses;
    ///      проверяем rows_affected == 1 (защита от гонки на max_uses).
    ///   3. Если inviter_user_id задан и у юзера ещё нет referrer_id — проставляем
    ///      его (immutable, set-once), как в signup-source семантике.
    ///   4. Безусловно гарантируем подписку на бесплатном плане — трафик в этой
    ///      системе приходит только от плана.
    ///
    /// Возвращает Ok(true) если код успешно списан, Ok(false) если код невалиден
    /// (не существует / истёк / исчерпан). Никогда не падает на отсутствии
    /// бесплатного плана — онбординг-грант деградирует мягко (skip, не rollback).
    ///
    /// `apply_signup_bonus` (как в bot /start) запускается best-effort ПОСЛЕ
    /// commit, чтобы сбой бонуса не откатывал списание кода.
    pub async fn redeem_enrollment_code(&self, user_id: i64, code: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        let outcome = self
            .redeem_enrollment_code_in_tx(&mut tx, user_id, code)
            .await?;

        let inviter = match outcome {
            Some(inv) => inv,
            None => {
                // Невалидный код: откатываем (ничего не делали) и сообщаем вызову.
                tx.rollback().await.ok();
                return Ok(false);
            }
        };

        tx.commit().await?;

        self.apply_enrollment_signup_bonus(inviter, user_id).await;

        Ok(true)
    }

    /// Атомарная регистрация email-аккаунта с обязательной попыткой списания
    /// enroll-кода: создание пользователя И redeem идут в ОДНОЙ транзакции.
    ///
    /// Решает major-1: раньше create_email_user коммитил юзера в пул ДО redeem в
    /// отдельной транзакции. Если код оказывался невалидным/исчерпанным или redeem
    /// падал транзиентно, аккаунт уже существовал, а повтор с верным кодом упирался
    /// в 409 (email занят) — валидный код было НЕВОЗМОЖНО списать, онбординг-трафик
    /// терялся навсегда. Теперь обе операции в одной tx: при невалидном коде или
    /// сбое redeem откатывается ВСЁ, аккаунт не создаётся, и клиент может повторить.
    ///
    /// `code` — уже trimmed непустая строка (валидатор вызова гарантирует это).
    /// Возвращает `Ok(Some(user))` при успехе, `Ok(None)` если код невалиден
    /// (аккаунт НЕ создан), `Err` при сбое БД (аккаунт НЕ создан).
    pub async fn register_email_with_enroll(
        &self,
        email: &str,
        password_hash: &str,
        full_name: Option<&str>,
        referral_code: &str,
        code: &str,
    ) -> Result<Option<User>> {
        let mut tx = self.pool.begin().await?;

        // 1. Создаём пользователя в ЭТОЙ же транзакции (не в пуле). Если redeem
        //    ниже не пройдёт — INSERT откатится вместе со всем остальным.
        let user_id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO users (email, password_hash, full_name, referral_code, auth_provider, email_verified)
            VALUES ($1, $2, $3, $4, 'email', FALSE)
            RETURNING id::bigint
            "#,
        )
        .bind(email)
        .bind(password_hash)
        .bind(full_name)
        .bind(referral_code)
        .fetch_one(&mut *tx)
        .await
        .context("Failed to create email user (enroll tx)")?;

        // 2. Списываем код в той же tx. None => невалиден: откатываем всё, аккаунт
        //    не создаётся (клиент повторит с верным кодом — email ещё свободен).
        let outcome = self
            .redeem_enrollment_code_in_tx(&mut tx, user_id, code)
            .await?;
        let inviter = match outcome {
            Some(inv) => inv,
            None => {
                tx.rollback().await.ok();
                return Ok(None);
            }
        };

        // 3. Читаем созданного юзера ДО commit, чтобы вернуть его целиком.
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .context("Failed to load created email user (enroll tx)")?;

        tx.commit().await?;

        self.apply_enrollment_signup_bonus(inviter, user_id).await;

        Ok(Some(user))
    }

    /// Ядро списания enroll-кода ВНУТРИ переданной транзакции. Делает шаги 1-4
    /// (lock + условный инкремент used_count + signup-source referrer_id +
    /// онбординг-грант). НЕ коммитит и НЕ откатывает — это ответственность вызова.
    ///
    /// Возвращает:
    ///   - `Ok(Some(inviter_user_id_opt))` — код успешно списан в этой tx; вызов
    ///     должен закоммитить и затем применить referral signup-бонус для
    ///     `inviter_user_id_opt` (best-effort, после commit).
    ///   - `Ok(None)` — код невалиден/исчерпан; вызов должен откатить tx.
    async fn redeem_enrollment_code_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
        code: &str,
    ) -> Result<Option<Option<i64>>> {
        // 1. Лочим строку кода под предикатом валидности (нуллабельный expires_at).
        let enroll = sqlx::query_as::<_, caramba_db::models::store::EnrollmentCode>(
            "SELECT * FROM enrollment_codes \
             WHERE code = $1 \
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP) \
               AND used_count < max_uses \
             FOR UPDATE",
        )
        .bind(code)
        .fetch_optional(&mut **tx)
        .await?;

        let enroll = match enroll {
            Some(e) => e,
            None => return Ok(None),
        };

        // 2. Условный инкремент: повторная проверка used_count < max_uses под
        //    блокировкой. rows_affected == 0 значит, что параллельная транзакция
        //    исчерпала код между SELECT и UPDATE — трактуем как невалидный.
        let res = sqlx::query(
            "UPDATE enrollment_codes SET used_count = used_count + 1 \
             WHERE id = $1 AND used_count < max_uses",
        )
        .bind(enroll.id)
        .execute(&mut **tx)
        .await?;
        if res.rows_affected() != 1 {
            return Ok(None);
        }

        // 3. Signup-source атрибуция: проставляем referrer_id один раз (immutable),
        //    только если inviter задан и не является самим пользователем.
        if let Some(inviter_id) = enroll.inviter_user_id
            && inviter_id != user_id
        {
            sqlx::query(
                "UPDATE users SET referrer_id = $1 \
                     WHERE id = $2 AND referrer_id IS NULL",
            )
            .bind(inviter_id)
            .bind(user_id)
            .execute(&mut **tx)
            .await?;
        }

        // 4. Бесплатная подписка — БЕЗУСЛОВНО.
        //
        //    Трафик в этой системе приходит только от плана, поэтому регистрация
        //    обязана посадить человека на план с `is_free`. Раньше здесь стоял
        //    одноразовый headroom за настройкой `onboarding_traffic_mb`, а рядом
        //    — плоский бонус за регистрацию; вместе они выдавали трафик человеку
        //    вообще без подписки, то есть в обход той самой сущности, которая
        //    трафиком управляет. Обоих больше нет.
        //
        //    Идемпотентно (живая подписка на бесплатном плане — no-op) и мягко
        //    деградирует: бесплатный план не настроен — warn и пропуск, но
        //    регистрация не откатывается. Внутри транзакции намеренно: падение
        //    между commit и post-commit шагом оставило бы человека без доступа.
        self.ensure_free_plan_subscription_tx(tx, user_id).await?;

        // 5. Реферальный бонус трафиком для ПРИГЛАШЁННОГО (referee). Независим от
        //    денежной модели (скидка на первую покупку) — 0 = выключено.
        //    Сторона пригласившего начисляется позже, в момент первой оплаты
        //    (referral_service::apply_first_purchase_reward).
        if let Some(inviter_id) = enroll.inviter_user_id
            && inviter_id != user_id
        {
            let referee_bonus_mb = crate::services::bonus_traffic::setting_mb_tx(
                tx,
                crate::services::bonus_traffic::SETTING_REFERRAL_BONUS_MB_REFEREE,
            )
            .await?;
            if referee_bonus_mb > 0 {
                crate::services::bonus_traffic::grant_tx(
                    tx,
                    user_id,
                    crate::services::bonus_traffic::SOURCE_REFERRAL_REFEREE,
                    &inviter_id.to_string(),
                    referee_bonus_mb,
                    Some("referral: signed up via invite"),
                )
                .await?;
            }
        }

        Ok(Some(enroll.inviter_user_id))
    }

    /// Реферальный signup-бонус (как в bot /start) — best-effort, ПОСЛЕ commit.
    /// Сам кредит идемпотентен (referral_bonuses гард), поэтому повторов не боимся;
    /// сбой не должен откатывать уже зафиксированное списание кода.
    async fn apply_enrollment_signup_bonus(&self, inviter: Option<i64>, user_id: i64) {
        if let Some(inviter_id) = inviter
            && inviter_id != user_id
            && let Err(e) =
                ReferralService::apply_signup_bonus(&self.pool, inviter_id, user_id).await
        {
            tracing::warn!(
                inviter_id,
                user_id,
                error = %e,
                "enrollment: failed to apply referral signup bonus (non-fatal)"
            );
        }
    }

    /// Гарантирует, что у пользователя есть подписка на бесплатном плане.
    ///
    /// Два вызывающих сценария, и оба про одно: человек не должен оставаться без
    /// подписки, потому что без неё его нет ни в одном конфиге ноды — он не может
    /// подключиться и, значит, не может дойти до экрана оплаты.
    ///
    ///   * РЕГИСТРАЦИЯ (`redeem_enrollment_code_in_tx`) — трафик приходит только
    ///     от плана, поэтому новый аккаунт сразу садится на бесплатный.
    ///   * ИСТЕЧЕНИЕ платной подписки (по сроку или по трафику) — откат на
    ///     бесплатный, ради чего этот план в первую очередь и существует.
    ///
    /// Идемпотентно и безопасно к гонкам по смыслу операций:
    ///   * есть активная ПЛАТНАЯ подписка (например, вторая) — не трогаем;
    ///   * бесплатный план не настроен — мягкий пропуск с warn;
    ///   * подписка на бесплатном плане уже активна/pending/throttled — no-op;
    ///   * строка есть, но 'expired' — реактивируем её, а не плодим дубль
    ///     (иначе у юзера накапливались бы бесплатные подписки со свежей квотой);
    ///   * строки нет — создаём с expires_at в 9999 году, как везде.
    ///
    /// used_traffic намеренно НЕ обнуляется при реактивации: это был бы подарок
    /// в обход суточной квоты. Если трафик исчерпан, суточное пополнение
    /// (monitoring::daily_traffic_topup) вернёт подписку в строй само.
    ///
    /// Возвращает `Some(plan_id)`, если после вызова подписка есть и её нужно
    /// раскатить по нодам (создали или реактивировали), иначе `None`.
    pub async fn ensure_free_plan_subscription_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
    ) -> Result<Option<i64>> {
        let has_paid: bool = sqlx::query_scalar(
            "SELECT EXISTS( \
               SELECT 1 FROM subscriptions s \
               JOIN plans p ON p.id = s.plan_id \
               WHERE s.user_id = $1 AND s.status = 'active' \
                 AND COALESCE(p.is_free, FALSE) = FALSE)",
        )
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?;
        if has_paid {
            return Ok(None);
        }

        let free_plan_id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM plans WHERE is_free = TRUE AND is_active = TRUE LIMIT 1",
        )
        .fetch_optional(&mut **tx)
        .await?;
        let Some(plan_id) = free_plan_id else {
            tracing::warn!(
                user_id,
                "free plan: none configured and active, user is left without access"
            );
            return Ok(None);
        };

        // Уже живая бесплатная подписка — ничего делать не нужно.
        let live: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM subscriptions \
             WHERE user_id = $1 AND plan_id = $2 AND status IN ('active', 'pending', 'throttled') \
             LIMIT 1",
        )
        .bind(user_id)
        .bind(plan_id)
        .fetch_optional(&mut **tx)
        .await?;
        if live.is_some() {
            return Ok(None);
        }

        // Активные строки снимаем ДО выдачи: их вытеснит либо общий метод, либо
        // триггер trg_subscriptions_single_active, но лизы устройств перенести
        // некому, а список после выдачи будет уже пуст. Платных строк здесь по
        // построению нет — выше стоит ранний выход по has_paid.
        let previous_active: Vec<i64> =
            sqlx::query_as::<_, Subscription>(ACTIVE_SUBSCRIPTIONS_FOR_UPDATE_SQL)
                .bind(user_id)
                .fetch_all(&mut **tx)
                .await?
                .into_iter()
                .map(|s| s.id)
                .collect();

        // Была, но истекла (или её вытеснила платная) — поднимаем ту же строку.
        // 'superseded' здесь наравне с 'expired': именно так бесплатная подписка
        // теперь выглядит, пока человек платит, и именно её нужно вернуть, когда
        // платная закончилась. Заодно лечим vless_uuid: строки, созданные до
        // того, как этот путь начал его выставлять, лежат с NULL и невидимы для
        // генерации конфигов нод — «восстановленная» подписка без него осталась
        // бы неподключаемой навсегда.
        let reactivated: Option<i64> = sqlx::query_scalar(
            "UPDATE subscriptions \
             SET status = 'active', expires_at = '9999-12-31 23:59:59+00', \
                 vless_uuid = COALESCE(NULLIF(vless_uuid, ''), gen_random_uuid()::TEXT) \
             WHERE id = ( \
                 SELECT id FROM subscriptions \
                 WHERE user_id = $1 AND plan_id = $2 AND status IN ('expired', 'superseded') \
                 ORDER BY id DESC LIMIT 1) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(plan_id)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(sub_id) = reactivated {
            let losers: Vec<i64> = previous_active
                .iter()
                .copied()
                .filter(|id| *id != sub_id)
                .collect();
            supersede_subscriptions_tx(tx, &losers, sub_id).await?;
            tracing::info!(
                user_id,
                plan_id,
                subscription_id = sub_id,
                "free plan: restored the subscription"
            );
            return Ok(Some(plan_id));
        }

        // Совсем нет — создаём. Конкурентная гонка двух свипов больше не может
        // оставить вторую активную строку: частичный уникальный индекс
        // uq_subscriptions_single_active не даст её зафиксировать, проигравшая
        // транзакция откатится и следующий вызов увидит уже готовую подписку.
        //
        // ИНВАРИАНТ: vless_uuid обязателен. Генерация конфигов нод
        // (orchestration_service) выбирает s.vless_uuid и МОЛЧА пропускает
        // подписки, где он NULL. Строка без него существует в базе, показывается
        // в кабинете и при этом не пускает ни в один inbound.
        let sub_id: i64 = sqlx::query_scalar(
            "INSERT INTO subscriptions \
             (user_id, plan_id, status, expires_at, vless_uuid, subscription_uuid, used_traffic, activated_at) \
             VALUES ($1, $2, 'active', '9999-12-31 23:59:59+00', gen_random_uuid()::TEXT, gen_random_uuid()::TEXT, 0, CURRENT_TIMESTAMP) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(plan_id)
        .fetch_one(&mut **tx)
        .await?;
        supersede_subscriptions_tx(tx, &previous_active, sub_id).await?;
        tracing::info!(
            user_id,
            plan_id,
            subscription_id = sub_id,
            "free plan: granted the subscription"
        );
        Ok(Some(plan_id))
    }

    /// Версия на собственной транзакции — для путей истечения и мониторинга,
    /// которым нечего разделять с вызывающим кодом. Вся логика живёт в
    /// `ensure_free_plan_subscription_tx`; здесь только рамка транзакции, чтобы
    /// решение «что считается живой бесплатной подпиской» существовало в одном
    /// месте и не разъезжалось между регистрацией и откатом после истечения.
    pub async fn ensure_free_plan_subscription(&self, user_id: i64) -> Result<Option<i64>> {
        let mut tx = self.pool.begin().await?;
        let outcome = self
            .ensure_free_plan_subscription_tx(&mut tx, user_id)
            .await?;
        tx.commit().await?;
        Ok(outcome)
    }

    pub async fn get_family_members(&self, parent_id: i64) -> Result<Vec<User>> {
        self.user_repo.get_by_parent_id(parent_id).await
    }

    pub async fn set_user_parent(&self, user_id: i64, parent_id: Option<i64>) -> Result<()> {
        self.user_repo.set_parent_id(user_id, parent_id).await?;
        if let Some(pid) = parent_id {
            self.sync_family_subscriptions(pid).await?;
        }
        Ok(())
    }

    /// Best-effort propagation of parent subscription to family children.
    /// Always called AFTER the parent's transaction has committed, and is
    /// invoked as `let _ = sync_family_subscriptions(..)` from non-critical
    /// paths so a sync failure leaves children stale but never rolls the
    /// parent's purchase back. Internal commits happen in this function's
    /// own transaction; on failure children stay on their previous state
    /// and will resync next time anything triggers this for that parent.
    pub async fn sync_family_subscriptions(&self, parent_id: i64) -> Result<()> {
        // Читаем данные до транзакции — эти запросы только на чтение
        let parent_sub = self.sub_repo.get_active_by_user(parent_id).await?;
        let children = self.get_family_members(parent_id).await?;
        if children.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        if let Some(psub) = parent_sub {
            for child in children {
                // Читаем дочернюю подписку внутри транзакции для согласованности,
                // тем же единым правилом выбора активной, что и везде.
                let child_sub = sqlx::query_as::<_, Subscription>(
                    caramba_db::repositories::subscription_repo::ACTIVE_SUBSCRIPTION_BY_USER_SQL,
                )
                .bind(child.id)
                .fetch_optional(&mut *tx)
                .await?;

                if let Some(csub) = child_sub {
                    if csub.note.as_deref() == Some("Family") || csub.plan_id == psub.plan_id {
                        // Обновляем семейную подписку в рамках транзакции
                        sqlx::query(
                            "UPDATE subscriptions SET expires_at = $1, plan_id = $2, node_id = $3, status = 'active', note = 'Family' WHERE id = $4"
                        )
                        .bind(psub.expires_at)
                        .bind(psub.plan_id)
                        .bind(psub.node_id)
                        .bind(csub.id)
                        .execute(&mut *tx)
                        .await?;
                    }
                } else {
                    // У ребёнка активной подписки нет — выдаём семейную общим
                    // методом, чтобы и здесь действовал один инвариант.
                    activate_or_replace_subscription_tx(
                        &mut tx,
                        SubscriptionGrant {
                            user_id: child.id,
                            plan_id: psub.plan_id,
                            expiry: SubscriptionExpiry::Exactly(psub.expires_at),
                            status: "active",
                            note: Some("Family"),
                            is_trial: false,
                            node_id: psub.node_id,
                        },
                    )
                    .await?;
                }
            }
        } else {
            for child in children {
                // Истекаем семейные подписки в рамках транзакции
                sqlx::query(
                    "UPDATE subscriptions SET status = 'expired' WHERE user_id = $1 AND note = 'Family' AND status = 'active'"
                )
                .bind(child.id)
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn increment_warning_count(&self, user_id: i64) -> Result<()> {
        self.user_repo.increment_warning_count(user_id).await?;
        Ok(())
    }

    pub async fn ban_user(&self, user_id: i64) -> Result<()> {
        let user = self
            .user_repo
            .get_by_id(user_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not found"))?;
        self.user_repo
            .update_profile(user_id, user.balance, true, user.referral_code.as_deref())
            .await?;
        Ok(())
    }

    pub async fn update_user_language(&self, user_id: i64, lang: &str) -> Result<()> {
        self.user_repo.update_language(user_id, lang).await?;
        Ok(())
    }

    pub async fn update_last_bot_msg_id(&self, user_id: i64, msg_id: i64) -> Result<()> {
        self.user_repo
            .update_last_bot_msg_id(user_id, msg_id)
            .await?;
        Ok(())
    }

    pub async fn add_bot_message_to_history(
        &self,
        user_id: i64,
        chat_id: i64,
        message_id: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO bot_chat_history (user_id, chat_id, message_id) VALUES ($1, $2, $3)",
        )
        .bind(user_id)
        .bind(chat_id)
        .bind(message_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn cleanup_bot_history(
        &self,
        user_id: i64,
        keep_count: i64,
    ) -> Result<Vec<(i64, i64)>> {
        let ids_to_delete: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT id, chat_id, message_id FROM bot_chat_history 
             WHERE user_id = $1 
             ORDER BY created_at DESC 
             OFFSET $2",
        )
        .bind(user_id)
        .bind(keep_count)
        .fetch_all(&self.pool)
        .await?;

        if ids_to_delete.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<i64> = ids_to_delete.iter().map(|(id, _, _)| *id).collect();
        sqlx::query("DELETE FROM bot_chat_history WHERE id = ANY($1)")
            .bind(&ids)
            .execute(&self.pool)
            .await?;

        Ok(ids_to_delete
            .into_iter()
            .map(|(_, chat_id, msg_id)| (chat_id, msg_id))
            .collect())
    }

    pub async fn update_user_terms(&self, user_id: i64) -> Result<()> {
        self.user_repo.update_terms_accepted(user_id).await?;
        Ok(())
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let res = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(res)
    }

    pub async fn update_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query("INSERT INTO settings (key, value) VALUES ($1, $2) ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn purchase_plan(
        &self,
        user_id: i64,
        duration_id: i64,
        as_gift: bool,
    ) -> Result<PurchaseResult> {
        let mut tx = self.pool.begin().await?;

        // FOR UPDATE блокирует строку пользователя на время транзакции,
        // предотвращая гонку при параллельных покупках (баланс не уйдёт в минус)
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        let duration = sqlx::query_as::<_, caramba_db::models::store::PlanDuration>(
            "SELECT * FROM plan_durations WHERE id = $1",
        )
        .bind(duration_id)
        .fetch_one(&mut *tx)
        .await?;

        if user.balance < duration.price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        // Проверяем, является ли выбранный план пробным (is_trial).
        // Если да — пользователь может воспользоваться пробным периодом только один раз.
        let plan_is_trial: Option<bool> =
            sqlx::query_scalar("SELECT is_trial FROM plans WHERE id = $1")
                .bind(duration.plan_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();

        if plan_is_trial.unwrap_or(false) && user.trial_used.unwrap_or(false) {
            return Err(anyhow::anyhow!(
                "Trial already used. You can only activate the trial period once."
            ));
        }

        sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
            .bind(duration.price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        // Фиксируем использование пробного периода внутри транзакции — чтобы при
        // откате флаг не остался установленным. Порядок с выдачей подписки здесь
        // не важен, важна только атомарность.
        if plan_is_trial.unwrap_or(false) {
            sqlx::query("UPDATE users SET trial_used = TRUE, trial_used_at = NOW() WHERE id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        }

        // Покупка подарка НЕ создаёт подписку покупателю. Раньше здесь
        // вставлялась pending-строка, которая тут же в этой же транзакции
        // удалялась ради gift-кода; с инвариантом «одна активная подписка» такой
        // промежуточный шаг стал бы прямо вредным — покупка подарка другу
        // вытесняла бы собственную подписку покупателя.
        if as_gift {
            let gift_code = format!(
                "CARAMBA-GIFT-{}",
                Uuid::new_v4()
                    .to_string()
                    .split('-')
                    .next()
                    .unwrap_or("CODE")
                    .to_uppercase()
            );

            sqlx::query(
                "INSERT INTO gift_codes (code, plan_id, duration_days, created_by_user_id) VALUES ($1, $2, $3, $4)"
            )
            .bind(&gift_code)
            .bind(duration.plan_id)
            .bind(duration.duration_days)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            let _ =
                crate::services::analytics_service::AnalyticsService::track_order(&self.pool).await;
            // Логируем активность через pool после коммита транзакции
            let _ = ActivityService::log(
                &self.pool,
                "Gift Purchase",
                &format!(
                    "Purchased gift code for plan (Duration ID: {})",
                    duration_id
                ),
            )
            .await;
            return Ok(PurchaseResult::GiftCode(gift_code));
        }

        // License gate (P4, contract E): на инстансе с ручным одобрением новая
        // подписка ждёт админа в 'pending' и никого не вытесняет; на Pro сразу
        // 'active'.
        let limits = crate::license::effective_limits_from_pool(&self.pool).await;
        let purchase_status = crate::license::initial_subscription_status(&limits);

        // Покупка того же тарифа — продление уже активной строки, а не вторая
        // подписка: иначе у человека появлялась вторая активная строка со своим
        // vless_uuid, и ранее выданные ссылки переставали соответствовать той
        // подписке, которую обслуживает система.
        let sub = activate_or_replace_subscription_tx(
            &mut tx,
            SubscriptionGrant {
                user_id,
                plan_id: duration.plan_id,
                expiry: SubscriptionExpiry::AddDays(duration.duration_days as i64),
                status: purchase_status,
                note: None,
                is_trial: plan_is_trial.unwrap_or(false),
                node_id: None,
            },
        )
        .await?;

        tx.commit().await?;
        let _ = crate::services::analytics_service::AnalyticsService::track_order(&self.pool).await;
        let _ = ActivityService::log_tx(
            &self.pool,
            Some(user_id),
            "Plan Purchase",
            &format!("Purchased plan (Duration ID: {})", duration_id),
        )
        .await;

        // Fix: Clone Arc inside lock, then await outside to avoid holding std::sync::RwLock across await
        let orch_opt = {
            if let Ok(lock) = self.orchestration_service.read() {
                lock.clone()
            } else {
                None
            }
        };

        if let Some(orch) = orch_opt
            && let Some(node_id) = sub.node_id
        {
            let _ = orch.notify_node_update(node_id).await;
        }

        Ok(PurchaseResult::Subscription(sub))
    }

    pub async fn purchase_product_with_balance(
        &self,
        user_id: i64,
        product_id: i64,
    ) -> Result<caramba_db::models::store::Product> {
        let mut tx = self.pool.begin().await?;
        let user: User = sqlx::query_as("SELECT * FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
        let product: caramba_db::models::store::Product =
            sqlx::query_as("SELECT * FROM products WHERE id = $1")
                .bind(product_id)
                .fetch_one(&mut *tx)
                .await?;

        if user.balance < product.price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
            .bind(product.price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Product Purchase",
            &format!("Purchased product: {}", product.name),
        )
        .await;

        tx.commit().await?;
        Ok(product)
    }

    pub async fn activate_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        // Читаем подписку внутри транзакции — статус-чек и запись обновления атомарны
        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE id = $1 AND user_id = $2",
        )
        .bind(sub_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!("Subscription is not pending"));
        }

        let duration = sub.expires_at - sub.created_at;
        let new_expires_at = Utc::now() + duration;

        // Момент, когда pending-подписка получает доступ, — это и есть момент
        // вытеснения прежней активной: до одобрения она ничего не заменяла.
        // Список снимаем ДО апдейта, потому что триггер
        // trg_subscriptions_single_active успеет перевести эти строки в
        // 'superseded' сам, а лизы устройств он не переносит.
        let previous_active: Vec<i64> =
            sqlx::query_as::<_, Subscription>(ACTIVE_SUBSCRIPTIONS_FOR_UPDATE_SQL)
                .bind(user_id)
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .map(|s| s.id)
                .filter(|id| *id != sub_id)
                .collect();

        // Обновляем статус и дату истечения внутри той же транзакции
        sqlx::query(
            "UPDATE subscriptions SET status = $1, expires_at = $2, used_traffic = 0 WHERE id = $3",
        )
        .bind("active")
        .bind(new_expires_at)
        .bind(sub_id)
        .execute(&mut *tx)
        .await?;

        supersede_subscriptions_tx(&mut tx, &previous_active, sub_id).await?;

        // Перечитываем обновлённую запись внутри транзакции до коммита
        let updated_sub =
            sqlx::query_as::<_, Subscription>("SELECT * FROM subscriptions WHERE id = $1")
                .bind(sub_id)
                .fetch_one(&mut *tx)
                .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Subscription",
            &format!("User {} activated sub {}", user_id, sub_id),
        )
        .await;

        tx.commit().await?;

        let orch_opt = {
            if let Ok(lock) = self.orchestration_service.read() {
                lock.clone()
            } else {
                None
            }
        };

        if let Some(orch) = orch_opt
            && let Some(node_id) = updated_sub.node_id
        {
            let _ = orch.notify_node_update(node_id).await;
        }

        Ok(updated_sub)
    }

    pub async fn get_subscription(&self, sub_id: i64, user_id: i64) -> Result<Subscription> {
        let sub = self
            .sub_repo
            .get_by_id(sub_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;

        if sub.user_id != user_id {
            return Err(anyhow::anyhow!("Unauthorized access to subscription"));
        }

        Ok(sub)
    }

    pub async fn convert_subscription_to_gift(&self, sub_id: i64, user_id: i64) -> Result<String> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(sub_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Subscription not found"))?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!(
                "Only pending subscriptions can be converted to gifts"
            ));
        }

        let duration = sub.expires_at - sub.created_at;
        let duration_days = duration.num_days() as i32;

        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

        let code = format!(
            "CARAMBA-GIFT-{}",
            Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("CODE")
                .to_uppercase()
        );

        sqlx::query(
            "INSERT INTO gift_codes (code, plan_id, duration_days, created_by_user_id) VALUES ($1, $2, $3, $4)"
        )
        .bind(&code)
        .bind(sub.plan_id)
        .bind(duration_days)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(code)
    }

    /// NOT THE LIVE GIFT-CODE PATH. Runtime gift/promo redemption runs through
    /// `PromoService::redeem_code` (which carries the manual_approval gate). This
    /// method has no callers and already inserts 'pending'; do not treat it as
    /// live manual_approval coverage.
    pub async fn redeem_gift_code(&self, user_id: i64, code: &str) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let gift_code_opt = sqlx::query_as::<_, caramba_db::models::store::GiftCode>(
            "SELECT * FROM gift_codes
             WHERE code = $1
               AND redeemed_by_user_id IS NULL
               AND COALESCE(status, 'active') = 'active'
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
             FOR UPDATE",
        )
        .bind(code)
        .fetch_optional(&mut *tx)
        .await?;

        let gift_code =
            gift_code_opt.ok_or_else(|| anyhow::anyhow!("Invalid or already redeemed code"))?;

        let days = gift_code
            .duration_days
            .ok_or_else(|| anyhow::anyhow!("Gift code invalid (no duration)"))?;
        let plan_id = gift_code
            .plan_id
            .ok_or_else(|| anyhow::anyhow!("Gift code invalid (no plan)"))?;

        let sub = activate_or_replace_subscription_tx(
            &mut tx,
            SubscriptionGrant {
                user_id,
                plan_id,
                expiry: SubscriptionExpiry::Exactly(Utc::now() + Duration::days(days as i64)),
                status: "pending",
                note: None,
                is_trial: false,
                node_id: None,
            },
        )
        .await?;

        sqlx::query("UPDATE gift_codes SET redeemed_by_user_id = $1, redeemed_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(user_id)
            .bind(gift_code.id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(sub)
    }

    pub async fn transfer_subscription(
        &self,
        sub_id: i64,
        current_user_id: i64,
        target_username: &str,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;

        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE id = $1 AND user_id = $2 FOR UPDATE",
        )
        .bind(sub_id)
        .bind(current_user_id)
        .fetch_one(&mut *tx)
        .await?;

        if sub.status != "pending" {
            return Err(anyhow::anyhow!(
                "Only pending subscriptions can be transferred"
            ));
        }

        let target_user = sqlx::query_as::<_, caramba_db::models::store::User>(
            "SELECT * FROM users WHERE username = $1",
        )
        .bind(target_username.trim_start_matches('@'))
        .fetch_optional(&mut *tx)
        .await?;

        let target_user = target_user.ok_or_else(|| {
            anyhow::anyhow!("Target user not found. They must start the bot first.")
        })?;

        if target_user.id == current_user_id {
            return Err(anyhow::anyhow!("Cannot transfer to yourself"));
        }

        let updated_sub = sqlx::query_as::<_, Subscription>(
            r#"
            UPDATE subscriptions 
            SET user_id = $1 
            WHERE id = $2 
            RETURNING *
            "#,
        )
        .bind(target_user.id)
        .bind(sub_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(updated_sub)
    }

    pub async fn admin_delete_subscription(&self, sub_id: i64) -> Result<()> {
        self.sub_repo.delete(sub_id).await?;
        Ok(())
    }

    pub async fn delete_subscription(&self, sub_id: i64, user_id: i64) -> Result<()> {
        let _sub = self.get_subscription(sub_id, user_id).await?;
        self.sub_repo.delete(sub_id).await?;
        Ok(())
    }

    pub async fn admin_refund_subscription(&self, sub_id: i64, amount: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Получаем подписку внутри транзакции с блокировкой строки
        let sub = sqlx::query_as::<_, Subscription>(
            "SELECT * FROM subscriptions WHERE id = $1 FOR UPDATE",
        )
        .bind(sub_id)
        .fetch_optional(&mut *tx)
        .await?
        .context("Subscription not found")?;

        // Удаляем подписку внутри транзакции — атомарно с возвратом баланса.
        // Ранее sub_repo.delete() использовал pool (не tx), поэтому при откате
        // транзакции подписка уже была удалена, а баланс не возвращён.
        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(sub.user_id)
            .execute(&mut *tx)
            .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(sub.user_id),
            "Refund",
            &format!("Refunded sub {} (Amt: {})", sub_id, amount),
        )
        .await;

        tx.commit().await?;
        Ok(())
    }

    pub async fn admin_extend_subscription(&self, sub_id: i64, days: i32) -> Result<()> {
        let user_id: i64 = sqlx::query_scalar("UPDATE subscriptions SET expires_at = expires_at + ($1 * interval '1 day') WHERE id = $2 RETURNING user_id")
            .bind(days)
            .bind(sub_id)
            .fetch_one(&self.pool)
            .await
            .context("Failed to extend subscription")?;

        let _ = self.sync_family_subscriptions(user_id).await;
        Ok(())
    }

    pub async fn admin_gift_subscription(
        &self,
        user_id: i64,
        plan_id: i64,
        duration_days: i32,
    ) -> Result<Subscription> {
        self.admin_gift_subscription_with_note(user_id, plan_id, duration_days, None)
            .await
    }

    /// То же самое, но с пометкой в `subscriptions.note`.
    ///
    /// Пометка нужна тем выдачам, которые потом придётся отличать от ручного
    /// подарка админа: сейчас это подарок при регистрации
    /// (`welcome_gift::NOTE`). Отдельный метод, а не новый аргумент у старого,
    /// чтобы не переписывать существующие вызовы ради `None`.
    pub async fn admin_gift_subscription_with_note(
        &self,
        user_id: i64,
        plan_id: i64,
        duration_days: i32,
        note: Option<&str>,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;
        let active_nodes = self.node_repo.get_active_node_ids().await?;
        let node_id = active_nodes
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No active nodes available"))?;

        // Подарок админа — точный срок от момента выдачи, а не продление: так
        // это и читается в админке («выдать N дней»). Общий метод при этом сам
        // погасит прежнюю активную подписку и перенесёт устройства.
        let sub = activate_or_replace_subscription_tx(
            &mut tx,
            SubscriptionGrant {
                user_id,
                plan_id,
                expiry: SubscriptionExpiry::Exactly(
                    Utc::now() + Duration::days(duration_days as i64),
                ),
                status: "active",
                note,
                is_trial: false,
                node_id: Some(node_id),
            },
        )
        .await?;

        tx.commit().await?;
        let _ = self.sync_family_subscriptions(user_id).await;

        // Trigger Sync
        let orch_opt = {
            if let Ok(lock) = self.orchestration_service.read() {
                lock.clone()
            } else {
                None
            }
        };

        if let Some(orch) = orch_opt
            && let Some(node_id) = sub.node_id
        {
            let _ = orch.notify_node_update(node_id).await;
        }

        Ok(sub)
    }

    pub async fn extend_subscription(
        &self,
        user_id: i64,
        duration_id: i64,
    ) -> Result<Subscription> {
        let mut tx = self.pool.begin().await?;
        // FOR UPDATE locks the user row for the life of the tx so two concurrent
        // extends can't both read the same balance and double-spend it negative
        // (mirrors purchase_plan / checkout_cart in this file).
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
        let duration =
            sqlx::query_as::<_, PlanDuration>("SELECT * FROM plan_durations WHERE id = $1")
                .bind(duration_id)
                .fetch_one(&mut *tx)
                .await?;

        if user.balance < duration.price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        // Defense-in-depth: conditional debit so the balance can never go
        // negative even if the guard above ever races a concurrent writer.
        let debited =
            sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2 AND balance >= $1")
                .bind(duration.price)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        if debited.rows_affected() != 1 {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }

        let sub = self
            .extend_subscription_with_duration_internal(user_id, &duration, &mut tx)
            .await?;
        tx.commit().await?;
        Ok(sub)
    }

    async fn extend_subscription_with_duration_internal(
        &self,
        user_id: i64,
        duration: &caramba_db::models::store::PlanDuration,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Subscription> {
        // Раньше здесь жила собственная развилка «тот же план / другой план», и
        // ветка «другой план» вставляла вторую строку, оставляя старую активной
        // навсегда. Теперь решение принимает общий метод — вместе с переносом
        // лиз устройств, которых эта развилка не знала.
        let sub = activate_or_replace_subscription_tx(
            tx,
            SubscriptionGrant {
                user_id,
                plan_id: duration.plan_id,
                expiry: SubscriptionExpiry::AddDays(duration.duration_days as i64),
                status: "active",
                note: None,
                is_trial: false,
                node_id: None,
            },
        )
        .await?;

        let _ = self.sync_family_subscriptions(user_id).await;
        Ok(sub)
    }

    pub async fn get_user_gift_codes(&self, user_id: i64) -> Result<Vec<GiftCode>> {
        sqlx::query_as::<_, GiftCode>(
            "SELECT * FROM gift_codes
             WHERE created_by_user_id = $1
               AND redeemed_by_user_id IS NULL
               AND COALESCE(status, 'active') = 'active'
               AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP)
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user gift codes")
    }

    pub async fn update_subscription_note(&self, sub_id: i64, note: String) -> Result<()> {
        sqlx::query("UPDATE subscriptions SET note = $1 WHERE id = $2")
            .bind(note)
            .bind(sub_id)
            .execute(&self.pool)
            .await
            .context("Failed to update subscription note")?;
        Ok(())
    }

    /// Записывает платёж в таблицу payments.
    /// Возвращает `true` если запись была создана (новый платёж),
    /// `false` если запись уже существует — идемпотентный конфликт по (method, external_id).
    /// Повторный вызов с тем же external_id безопасен и не создаёт дубликат.
    pub async fn log_payment(
        &self,
        user_id: i64,
        method: &str,
        amount_cents: i64,
        external_id: Option<&str>,
        status: &str,
    ) -> Result<bool> {
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO payments (user_id, method, amount, external_id, status) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (method, external_id) WHERE external_id IS NOT NULL \
             DO NOTHING RETURNING id",
        )
        .bind(user_id)
        .bind(method)
        .bind(amount_cents)
        .bind(external_id)
        .bind(status)
        .fetch_optional(&self.pool)
        .await?;
        // inserted.is_some() → новая запись; None → уже существовала (duplicate)
        Ok(inserted.is_some())
    }

    pub async fn apply_referral_bonus(
        &self,
        pool: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: i64,
        amount_cents: i64,
        payment_id: Option<i64>,
    ) -> Result<Option<(i64, i64)>> {
        ReferralService::apply_referral_bonus(pool, user_id, amount_cents, payment_id).await
    }

    pub async fn create_category(
        &self,
        name: &str,
        description: Option<&str>,
        sort_order: Option<i32>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO categories (name, description, sort_order) VALUES ($1, $2, $3)")
            .bind(name)
            .bind(description)
            .bind(sort_order)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_category(&self, id: i64) -> Result<()> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE category_id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        if count > 0 {
            return Err(anyhow::anyhow!(
                "Cannot delete category with existing products"
            ));
        }
        sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_all_products(&self) -> Result<Vec<caramba_db::models::store::Product>> {
        sqlx::query_as::<_, caramba_db::models::store::Product>(
            "SELECT * FROM products ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch all products")
    }

    pub async fn create_product(
        &self,
        category_id: i64,
        name: &str,
        description: Option<&str>,
        price: i64,
        product_type: &str,
        content: Option<&str>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO products (category_id, name, description, price, product_type, content) VALUES ($1, $2, $3, $4, $5, $6)").bind(category_id).bind(name).bind(description).bind(price).bind(product_type).bind(content).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn delete_product(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM products WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_active_node_ids(&self) -> Result<Vec<i64>> {
        self.node_repo.get_active_node_ids().await
    }

    pub async fn update_user_referral_code(&self, user_id: i64, new_code: &str) -> Result<()> {
        self.user_repo
            .update_user_referral_code(user_id, new_code)
            .await
    }

    pub async fn get_user_subscriptions(
        &self,
        user_id: i64,
    ) -> Result<Vec<caramba_db::models::store::SubscriptionWithDetails>> {
        self.sub_repo.get_all_by_user(user_id).await
    }

    pub async fn get_referral_count(&self, user_id: i64) -> Result<i64> {
        ReferralService::get_referral_count(&self.pool, user_id).await
    }

    pub async fn get_subscription_active_ips(
        &self,
        sub_id: i64,
    ) -> Result<Vec<caramba_db::models::store::SubscriptionIpTracking>> {
        self.sub_repo.get_active_ips(sub_id).await
    }

    pub async fn get_subscription_device_limit(&self, sub_id: i64) -> Result<i32> {
        self.sub_repo
            .get_device_limit(sub_id)
            .await
            .map(|opt| opt.unwrap_or(0))
    }

    pub async fn get_active_plans(&self) -> Result<Vec<caramba_db::models::store::Plan>> {
        sqlx::query_as::<_, caramba_db::models::store::Plan>(
            "SELECT * FROM plans WHERE is_active = TRUE ORDER BY sort_order ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active plans")
    }

    pub async fn get_subscription_links(&self, sub_id: i64) -> Result<Vec<String>> {
        let sub = self
            .sub_repo
            .get_by_id(sub_id)
            .await?
            .context("Subscription not found")?;

        let nodes = self.get_user_nodes(sub.user_id).await?;
        let node_infos: Vec<crate::singbox::subscription_generator::NodeInfo> =
            nodes.iter().map(|n| n.into()).collect();

        let user_uuid = sub
            .vless_uuid
            .as_deref()
            .map(str::trim)
            .filter(|uuid| !uuid.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| sub.subscription_uuid.clone());

        // We need UserKeys
        let user_keys = crate::singbox::subscription_generator::UserKeys {
            user_uuid: user_uuid.clone(),
            hy2_password: user_uuid, // Fallback
            _awg_private_key: None,
        };

        let base64_config = crate::singbox::subscription_generator::generate_v2ray_config(
            &sub,
            &node_infos,
            &user_keys,
            &[],
        )?;

        use base64::Engine;
        let decoded = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(base64_config)
                .map_err(|e| anyhow::anyhow!("Decode failed: {}", e))?,
        )?;

        Ok(decoded.lines().map(|s| s.to_string()).collect())
    }

    pub async fn toggle_auto_renewal(&self, sub_id: i64) -> Result<bool> {
        let sub = self
            .sub_repo
            .get_by_id(sub_id)
            .await?
            .context("Subscription not found")?;
        let new_state = !sub.auto_renew.unwrap_or(false);
        sqlx::query("UPDATE subscriptions SET auto_renew = $1 WHERE id = $2")
            .bind(new_state)
            .bind(sub_id)
            .execute(&self.pool)
            .await?;
        Ok(new_state)
    }

    pub async fn kill_subscription_connections(&self, sub_id: i64) -> Result<()> {
        // Implementation depends on how connections are tracked.
        // For now, we can clear IP tracking as a way to signal reset (this is a placeholder for real killing)
        sqlx::query("DELETE FROM subscription_ip_tracking WHERE subscription_id = $1")
            .bind(sub_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn generate_subscription_file(&self, user_id: i64) -> Result<String> {
        let subs = self.get_user_subscriptions(user_id).await?;
        let mut config = serde_json::json!({
            "version": 2,
            "profiles": []
        });

        for sub in subs {
            let links = self
                .get_subscription_links(sub.sub.id)
                .await
                .unwrap_or_default();
            if let Some(profiles) = config["profiles"].as_array_mut() {
                profiles.push(serde_json::json!({
                    "name": sub.plan_name,
                    "links": links
                }));
            }
        }

        Ok(serde_json::to_string_pretty(&config)?)
    }

    pub async fn validate_promo(
        &self,
        code: &str,
    ) -> Result<Option<caramba_db::models::promo::PromoCode>> {
        sqlx::query_as::<_, caramba_db::models::promo::PromoCode>(
            "SELECT * FROM promo_codes WHERE code = $1 AND (expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP) AND current_uses < max_uses AND is_active = TRUE"
        ).bind(code).fetch_optional(&self.pool).await.context("Failed to validate promo code")
    }

    pub async fn checkout_cart(&self, user_id: i64) -> Result<Vec<String>> {
        let cart = self.get_user_cart(user_id).await?;
        if cart.is_empty() {
            return Err(anyhow::anyhow!("Cart is empty"));
        }
        let total_price: i64 = cart.iter().map(|item| item.price * item.quantity).sum();
        let mut tx = self.pool.begin().await?;
        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
        if user.balance < total_price {
            return Err(anyhow::anyhow!("Insufficient balance"));
        }
        sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
            .bind(total_price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        let order_id: i64 = sqlx::query_scalar("INSERT INTO orders (user_id, total_amount, status, paid_at) VALUES ($1, $2, 'paid', CURRENT_TIMESTAMP) RETURNING id")
            .bind(user_id).bind(total_price).fetch_one(&mut *tx).await?;
        for item in cart {
            sqlx::query("INSERT INTO order_items (order_id, product_id, quantity, price) VALUES ($1, $2, $3, $4)").bind(order_id).bind(item.product_id).bind(item.quantity).bind(item.price).execute(&mut *tx).await?;
        }
        sqlx::query("DELETE FROM cart_items WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Checkout",
            &format!("Checkout complete. Total: {}", total_price),
        )
        .await;
        tx.commit().await?;
        Ok(vec!["Order processed successfully".to_string()])
    }

    pub async fn get_user_cart(&self, user_id: i64) -> Result<Vec<CartItem>> {
        sqlx::query_as::<_, CartItem>(
            "SELECT c.id, c.user_id, c.product_id, c.quantity, p.name as product_name, p.price FROM cart_items c JOIN products p ON c.product_id = p.id WHERE c.user_id = $1"
        ).bind(user_id).fetch_all(&self.pool).await.context("Failed to fetch cart")
    }

    pub async fn add_to_cart(&self, user_id: i64, product_id: i64, quantity: i64) -> Result<()> {
        sqlx::query("INSERT INTO cart_items (user_id, product_id, quantity) VALUES ($1, $2, $3) ON CONFLICT(user_id, product_id) DO UPDATE SET quantity = cart_items.quantity + EXCLUDED.quantity")
            .bind(user_id).bind(product_id).bind(quantity).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn clear_cart(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM cart_items WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_product(&self, prod_id: i64) -> Result<caramba_db::models::store::Product> {
        sqlx::query_as::<_, caramba_db::models::store::Product>(
            "SELECT id, category_id, name, description, price, product_type, content, is_active, created_at FROM products WHERE id = $1"
        )
        .bind(prod_id)
        .fetch_one(&self.pool)
        .await
        .context("Failed to fetch product")
    }

    pub async fn get_categories(&self) -> Result<Vec<caramba_db::models::store::StoreCategory>> {
        sqlx::query_as::<_, caramba_db::models::store::StoreCategory>(
            "SELECT * FROM categories WHERE is_active = TRUE ORDER BY sort_order ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch categories")
    }

    pub async fn delete_user_session(&self, _user_id: i64) -> Result<()> {
        // AMBIGUOUS: в текущей архитектуре сессии хранятся в JWT/cookie без серверной таблицы.
        // Если будет добавлена таблица user_sessions — реализовать DELETE WHERE user_id = $1 LIMIT 1.
        Ok(())
    }

    pub async fn delete_all_user_sessions(&self, _user_id: i64) -> Result<()> {
        // AMBIGUOUS: аналогично delete_user_session — нет серверной таблицы сессий.
        // При добавлении таблицы user_sessions — DELETE WHERE user_id = $1.
        Ok(())
    }

    pub async fn get_user_referral_earnings(&self, user_id: i64) -> Result<i64> {
        ReferralService::get_user_referral_earnings(&self.pool, user_id).await
    }

    pub async fn set_user_referrer(&self, user_id: i64, code: &str) -> Result<()> {
        let referrer = self
            .user_repo
            .get_by_referral_code(code)
            .await?
            .context("Referrer not found")?;
        self.user_repo.set_referrer_id(user_id, referrer.id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    // ---- срок подписки ------------------------------------------------------

    /// Подарок и промокод дают ровно тот срок, который выписан — прошлая
    /// подписка на этот срок не влияет.
    #[test]
    fn an_exact_expiry_ignores_whatever_was_there_before() {
        let target = now() + Duration::days(30);
        assert_eq!(
            SubscriptionExpiry::Exactly(target).resolve(None, now()),
            target
        );
        assert_eq!(
            SubscriptionExpiry::Exactly(target).resolve(Some(now() + Duration::days(300)), now()),
            target
        );
    }

    /// Продление живой подписки прибавляется к её дате: оплаченные дни не
    /// сгорают. Именно этого не делала ветка «другой план» — она вместо
    /// продления заводила вторую строку.
    #[test]
    fn extending_a_live_subscription_adds_to_its_own_expiry() {
        let current = now() + Duration::days(10);
        assert_eq!(
            SubscriptionExpiry::AddDays(30).resolve(Some(current), now()),
            current + Duration::days(30)
        );
    }

    /// Продление давно истёкшей считается от «сейчас»: иначе подписка стала бы
    /// активной с датой в прошлом, и ближайшая проверка сроков снова погасила
    /// бы её — флап статуса и лишние перегенерации конфигов.
    #[test]
    fn extending_a_long_dead_subscription_starts_from_now() {
        let long_gone = now() - Duration::days(400);
        assert_eq!(
            SubscriptionExpiry::AddDays(30).resolve(Some(long_gone), now()),
            now() + Duration::days(30)
        );
        assert_eq!(
            SubscriptionExpiry::AddDays(30).resolve(None, now()),
            now() + Duration::days(30)
        );
    }

    // ---- инвариант виден в исходниках --------------------------------------
    //
    // Живой базы у тестов этого крейта нет (CI гоняет cargo test без Postgres),
    // поэтому «единственный путь выдачи» проверяется по тексту — в том же
    // стиле, что tests/free_plan_grant_guard.rs. Каждый обход общего метода
    // ломается молча: подписка создаётся, показывается в кабинете, а устройства
    // человека тихо отвязываются.

    /// Исходник без собственного тестового модуля: сами тесты цитируют SQL, и
    /// без отсечения счётчик запросов считал бы эти цитаты.
    fn panel_src(relative: &str) -> String {
        let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(relative);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        text.split("#[cfg(test)]").next().unwrap().to_string()
    }

    fn migration(name: &str) -> String {
        let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../libs/caramba-db/migrations")
            .join(name);
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }

    /// Пути выдачи подписок обязаны идти через общий метод. Свой INSERT в этих
    /// файлах — это ровно тот дубль активной подписки, который мы убрали.
    #[test]
    fn subscription_grants_do_not_write_their_own_insert() {
        for file in [
            "services/promo_service.rs",
            "handlers/api/bot.rs",
            "services/welcome_gift.rs",
        ] {
            let src = panel_src(file);
            let inserts = src.matches("INSERT INTO subscriptions").count();
            let expected = if file == "handlers/api/bot.rs" {
                // create_free_subscription — вторая, намеренно оставленная
                // реализация выдачи бесплатного плана: ей нужен другой контракт
                // ответа ({subscription_id, already_had_free}).
                1
            } else {
                0
            };
            assert_eq!(
                inserts, expected,
                "{file}: выдача подписки в обход activate_or_replace_subscription"
            );
        }
    }

    /// В самом store_service INSERT'ов подписки ровно два: общий
    /// insert_subscription_tx и выдача бесплатного плана (у неё свой контракт —
    /// вечный срок и реактивация старой строки).
    #[test]
    fn the_store_service_keeps_exactly_two_subscription_inserts() {
        let src = panel_src("services/store_service.rs");
        assert_eq!(
            src.matches("INSERT INTO subscriptions").count(),
            2,
            "в store_service появился новый путь создания подписки"
        );
    }

    /// Замена подписки без переноса лиз молча отвязывает все устройства
    /// пользователя: их отпечаток считается от subscription_id.
    #[test]
    fn superseding_carries_the_device_leases_over() {
        let src = panel_src("services/store_service.rs");
        let body = src
            .split("async fn supersede_subscriptions_tx")
            .nth(1)
            .expect("supersede_subscriptions_tx исчез — обновите тест");
        assert!(
            body.contains("UPDATE subscription_device_leases"),
            "вытеснение перестало переносить привязки устройств"
        );
        assert!(
            body.contains("STATUS_SUPERSEDED"),
            "вытесненная подписка обязана получать отдельный статус, а не 'expired'"
        );
    }

    /// Инвариант обязан держаться и на уровне БД: код — не единственный
    /// писатель (есть ручные правки и пути вне этой волны).
    #[test]
    fn the_migration_pins_the_invariant_in_the_database() {
        let sql = migration("20260911140000_single_active_subscription.sql");
        assert!(
            sql.contains("CREATE UNIQUE INDEX IF NOT EXISTS uq_subscriptions_single_active"),
            "из миграции пропал уникальный индекс одной активной подписки"
        );
        assert!(
            sql.contains("WHERE status = 'active'"),
            "индекс перестал быть частичным — он запретил бы и историю подписок"
        );
        assert!(
            sql.contains("trg_subscriptions_single_active")
                && sql.contains("trg_subscriptions_carry_leases"),
            "исчезла страховка-триггер: пути активации вне общего метода начнут падать на индексе"
        );
    }

    /// Бесплатный план возвращается после платной только если реактивация
    /// видит статус 'superseded' — именно в него теперь уходит бесплатная
    /// строка, когда человек платит.
    #[test]
    fn the_free_plan_comes_back_from_superseded_too() {
        let src = panel_src("services/store_service.rs");
        let grant = src
            .split("pub async fn ensure_free_plan_subscription_tx")
            .nth(1)
            .expect("ensure_free_plan_subscription_tx исчез — обновите тест");
        let reactivation = grant
            .split("pub async fn ensure_free_plan_subscription(")
            .next()
            .unwrap();
        assert!(
            reactivation.contains("status IN ('expired', 'superseded')"),
            "реактивация бесплатного плана не видит вытесненную строку: человек останется без доступа"
        );
    }
}
