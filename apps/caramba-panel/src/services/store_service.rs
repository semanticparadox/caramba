use crate::services::activity_service::ActivityService;
use crate::services::referral_service::ReferralService;
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use caramba_db::models::store::{CartItem, GiftCode, PlanDuration, Subscription, User};

use caramba_db::repositories::api_key_repo::ApiKeyRepository;
use caramba_db::repositories::node_repo::NodeRepository;
use caramba_db::repositories::subscription_repo::SubscriptionRepository;
use caramba_db::repositories::user_repo::UserRepository;

/// Результат покупки тарифа: подписка создана активной, либо создан подарочный код.
#[derive(Debug)]
pub enum PurchaseResult {
    Subscription(Subscription),
    GiftCode(String),
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

        // Была, но истекла — поднимаем ту же строку.
        let reactivated: Option<i64> = sqlx::query_scalar(
            "UPDATE subscriptions \
             SET status = 'active', expires_at = '9999-12-31 23:59:59+00' \
             WHERE user_id = $1 AND plan_id = $2 AND status = 'expired' \
             RETURNING id",
        )
        .bind(user_id)
        .bind(plan_id)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(sub_id) = reactivated {
            tracing::info!(
                user_id,
                plan_id,
                subscription_id = sub_id,
                "free plan: restored the subscription"
            );
            return Ok(Some(plan_id));
        }

        // Совсем нет — создаём. Конкурентная гонка двух свипов даст в худшем
        // случае вторую строку на том же плане; последующие вызовы её увидят
        // как 'active' и остановятся, а сама вторая строка безвредна (квота
        // считается по плану + бонусу пользователя, а не по числу строк).
        let sub_id: i64 = sqlx::query_scalar(
            "INSERT INTO subscriptions \
             (user_id, plan_id, status, expires_at, subscription_uuid, used_traffic, activated_at) \
             VALUES ($1, $2, 'active', '9999-12-31 23:59:59+00', gen_random_uuid()::TEXT, 0, CURRENT_TIMESTAMP) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(plan_id)
        .fetch_one(&mut **tx)
        .await?;
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
                // Читаем дочернюю подписку внутри транзакции для согласованности
                let child_sub = sqlx::query_as::<_, Subscription>(
                    "SELECT * FROM subscriptions WHERE user_id = $1 AND status = 'active' ORDER BY expires_at DESC LIMIT 1"
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
                    let vless_uuid = Uuid::new_v4().to_string();
                    let sub_uuid = Uuid::new_v4().to_string();
                    // Создаём семейную подписку в рамках транзакции
                    sqlx::query(
                        r#"INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status, note, created_at, is_trial)
                           VALUES ($1, $2, $3, $4, $5, $6, $7, CURRENT_TIMESTAMP, FALSE)"#
                    )
                    .bind(child.id)
                    .bind(psub.plan_id)
                    .bind(&vless_uuid)
                    .bind(&sub_uuid)
                    .bind(psub.expires_at)
                    .bind("active")
                    .bind("Family")
                    .execute(&mut *tx)
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

        let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
        let vless_uuid = Uuid::new_v4().to_string();
        let sub_uuid = Uuid::new_v4().to_string();

        // as_gift=true → подписка pending (будет конвертирована в код),
        // as_gift=false → подписка для конечного пользователя.
        //
        // License gate (P4, contract E): for a real end-user purchase (as_gift=false)
        // on a Free instance (manual_approval), the new sub stays 'pending' until an
        // admin approves; Pro -> auto-'active'. Reuses the existing pending/active
        // lifecycle and never touches existing subs. Gift purchases stay 'pending'
        // regardless (they are converted into a code, not handed to the buyer).
        let limits = crate::license::effective_limits_from_pool(&self.pool).await;
        let purchase_status = crate::license::initial_subscription_status(&limits);

        let sub = if as_gift {
            sqlx::query_as::<_, Subscription>(
                r#"
                INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status, is_trial)
                VALUES ($1, $2, $3, $4, $5, 'pending', $6)
                RETURNING *
                "#
            )
            .bind(user_id)
            .bind(duration.plan_id)
            .bind(&vless_uuid)
            .bind(&sub_uuid)
            .bind(expires_at)
            .bind(plan_is_trial.unwrap_or(false))
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query_as::<_, Subscription>(
                r#"
                INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status, is_trial, activated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
                RETURNING *
                "#
            )
            .bind(user_id)
            .bind(duration.plan_id)
            .bind(&vless_uuid)
            .bind(&sub_uuid)
            .bind(expires_at)
            .bind(purchase_status)
            .bind(plan_is_trial.unwrap_or(false))
            .fetch_one(&mut *tx)
            .await?
        };

        // Фиксируем использование пробного периода сразу после создания подписки,
        // ещё внутри транзакции — чтобы при откате флаг не остался установленным.
        if plan_is_trial.unwrap_or(false) {
            sqlx::query("UPDATE users SET trial_used = TRUE, trial_used_at = NOW() WHERE id = $1")
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        }

        // Если покупка как подарок — сразу конвертируем pending-подписку в gift code внутри той же транзакции.
        // Подписка удаляется, создаётся запись в gift_codes, транзакция фиксируется с кодом.
        if as_gift {
            sqlx::query("DELETE FROM subscriptions WHERE id = $1")
                .bind(sub.id)
                .execute(&mut *tx)
                .await?;

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
            .bind(sub.plan_id)
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

        // Обновляем статус и дату истечения внутри той же транзакции
        sqlx::query(
            "UPDATE subscriptions SET status = $1, expires_at = $2, used_traffic = 0 WHERE id = $3",
        )
        .bind("active")
        .bind(new_expires_at)
        .bind(sub_id)
        .execute(&mut *tx)
        .await?;

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

        let expires_at = Utc::now() + Duration::days(days as i64);
        let vless_uuid = Uuid::new_v4().to_string();
        let subscription_uuid = Uuid::new_v4().to_string();

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, vless_uuid, subscription_uuid, expires_at, status)
            VALUES ($1, $2, $3, $4, $5, 'pending')
            RETURNING *
            "#
        )
        .bind(user_id)
        .bind(plan_id)
        .bind(vless_uuid)
        .bind(subscription_uuid)
        .bind(expires_at)
        .fetch_one(&mut *tx)
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
        let mut tx = self.pool.begin().await?;
        let active_nodes = self.node_repo.get_active_node_ids().await?;
        let node_id = active_nodes
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("No active nodes available"))?;

        let vless_uuid = Uuid::new_v4().to_string();
        let sub_uuid = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::days(duration_days as i64);

        let sub = sqlx::query_as::<_, Subscription>(
            r#"
            INSERT INTO subscriptions (user_id, plan_id, node_id, vless_uuid, expires_at, status, subscription_uuid, created_at)
            VALUES ($1, $2, $3, $4, $5, 'active', $6, CURRENT_TIMESTAMP)
            RETURNING *
            "#
        )
        .bind(user_id).bind(plan_id).bind(node_id).bind(vless_uuid).bind(expires_at).bind(sub_uuid)
        .fetch_one(&mut *tx)
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
        // Все операции с подписками выполняются через tx-aware варианты репозитория,
        // чтобы списание баланса и изменение подписки были атомарны.
        let existing_sub = self.sub_repo.get_active_by_user_tx(tx, user_id).await?;

        let sub = if let Some(active_sub) = existing_sub {
            if active_sub.plan_id != duration.plan_id {
                let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
                let vless_uuid = Uuid::new_v4().to_string();
                let sub_uuid = Uuid::new_v4().to_string();
                let id = self
                    .sub_repo
                    .create_tx(
                        tx,
                        user_id,
                        duration.plan_id,
                        &vless_uuid,
                        &sub_uuid,
                        expires_at,
                        "active",
                        None,
                    )
                    .await?;
                self.sub_repo
                    .get_by_id_tx(tx, id)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Subscription {} not found after insert", id))?
            } else {
                let new_expires_at = if active_sub.expires_at > Utc::now() {
                    active_sub.expires_at + Duration::days(duration.duration_days as i64)
                } else {
                    Utc::now() + Duration::days(duration.duration_days as i64)
                };
                self.sub_repo
                    .update_expiry_tx(tx, active_sub.id, new_expires_at)
                    .await?;
                self.sub_repo
                    .get_by_id_tx(tx, active_sub.id)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Subscription {} not found after expiry update",
                            active_sub.id
                        )
                    })?
            }
        } else {
            let expires_at = Utc::now() + Duration::days(duration.duration_days as i64);
            let vless_uuid = Uuid::new_v4().to_string();
            let sub_uuid = Uuid::new_v4().to_string();
            let id = self
                .sub_repo
                .create_tx(
                    tx,
                    user_id,
                    duration.plan_id,
                    &vless_uuid,
                    &sub_uuid,
                    expires_at,
                    "active",
                    None,
                )
                .await?;
            self.sub_repo
                .get_by_id_tx(tx, id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Subscription {} not found after insert", id))?
        };

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
