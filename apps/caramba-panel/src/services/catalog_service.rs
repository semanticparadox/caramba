use crate::services::activity_service::ActivityService;
use anyhow::{Context, Result};
use caramba_db::models::store::{CartItem, Plan, PlanDuration, Product, StoreCategory};
use chrono::Utc;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct CatalogService {
    pool: PgPool,
}

impl CatalogService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // ============================================================
    // Per-payment-method price/currency overrides
    // ------------------------------------------------------------
    // When an override row exists for (target, provider) the checkout charges that
    // amount+currency; otherwise it falls back to the base price in USD. Amounts are
    // integer minor units (cents/kopecks), matching plan_durations.price / products.price.
    // ============================================================

    /// Resolve the effective `(plan_id, duration_days, amount, currency)` for a plan
    /// duration under a given provider. `duration_days` is carried through so the
    /// payment session metadata records exactly how long to extend the subscription,
    /// independent of the (possibly overridden) charged amount. `None` if missing.
    pub async fn resolve_duration_price(
        &self,
        duration_id: i64,
        provider: &str,
    ) -> Result<Option<(i64, i32, i64, String)>> {
        let base = sqlx::query_as::<_, (i64, i32, i64)>(
            "SELECT plan_id, duration_days, price FROM plan_durations WHERE id = $1",
        )
        .bind(duration_id)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch plan duration base price")?;

        let (plan_id, duration_days, base_price) = match base {
            Some(v) => v,
            None => return Ok(None),
        };

        let ov = sqlx::query_as::<_, (i64, String)>(
            "SELECT amount, currency FROM plan_duration_provider_prices WHERE duration_id = $1 AND provider = $2",
        )
        .bind(duration_id)
        .bind(provider)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);

        match ov {
            Some((amount, currency)) => Ok(Some((plan_id, duration_days, amount, currency))),
            None => Ok(Some((
                plan_id,
                duration_days,
                base_price,
                "USD".to_string(),
            ))),
        }
    }

    /// Resolve the effective amount+currency for a store order under a given provider.
    /// If the order has exactly one product line and an override exists for that
    /// product+provider, charges `override.amount * quantity` in the override currency.
    /// Otherwise falls back to the base `total_amount` in USD. `None` if order missing.
    pub async fn resolve_order_price(
        &self,
        order_id: i64,
        provider: &str,
    ) -> Result<Option<(i64, String)>> {
        let base = sqlx::query_as::<_, (i64,)>("SELECT total_amount FROM orders WHERE id = $1")
            .bind(order_id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch order total")?;
        let base_total = match base {
            Some((t,)) => t,
            None => return Ok(None),
        };

        let items = sqlx::query_as::<_, (i64, i64)>(
            "SELECT product_id, quantity FROM order_items WHERE order_id = $1",
        )
        .bind(order_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        if items.len() == 1 {
            let (product_id, quantity) = items[0];
            let ov = sqlx::query_as::<_, (i64, String)>(
                "SELECT amount, currency FROM product_provider_prices WHERE product_id = $1 AND provider = $2",
            )
            .bind(product_id)
            .bind(provider)
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None);
            if let Some((amount, currency)) = ov {
                return Ok(Some((amount * quantity.max(1), currency)));
            }
        }

        Ok(Some((base_total, "USD".to_string())))
    }

    /// All provider overrides for a plan duration, keyed by provider name.
    pub async fn list_duration_overrides(
        &self,
        duration_id: i64,
    ) -> HashMap<String, (i64, String)> {
        sqlx::query_as::<_, (String, i64, String)>(
            "SELECT provider, amount, currency FROM plan_duration_provider_prices WHERE duration_id = $1",
        )
        .bind(duration_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(p, a, c)| (p, (a, c)))
        .collect()
    }

    /// All provider overrides for a product, keyed by provider name.
    pub async fn list_product_overrides(&self, product_id: i64) -> HashMap<String, (i64, String)> {
        sqlx::query_as::<_, (String, i64, String)>(
            "SELECT provider, amount, currency FROM product_provider_prices WHERE product_id = $1",
        )
        .bind(product_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(p, a, c)| (p, (a, c)))
        .collect()
    }

    /// Insert or update a plan-duration override. `currency` is upper-cased.
    pub async fn upsert_duration_override(
        &self,
        duration_id: i64,
        provider: &str,
        amount: i64,
        currency: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO plan_duration_provider_prices (duration_id, provider, amount, currency, updated_at)
             VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)
             ON CONFLICT (duration_id, provider)
             DO UPDATE SET amount = EXCLUDED.amount, currency = EXCLUDED.currency, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(duration_id)
        .bind(provider)
        .bind(amount)
        .bind(currency.trim().to_uppercase())
        .execute(&self.pool)
        .await
        .context("Failed to upsert plan duration override")?;
        Ok(())
    }

    /// Remove a plan-duration override (reverts to base USD price).
    pub async fn delete_duration_override(&self, duration_id: i64, provider: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM plan_duration_provider_prices WHERE duration_id = $1 AND provider = $2",
        )
        .bind(duration_id)
        .bind(provider)
        .execute(&self.pool)
        .await
        .context("Failed to delete plan duration override")?;
        Ok(())
    }

    /// Insert or update a product override. `currency` is upper-cased.
    pub async fn upsert_product_override(
        &self,
        product_id: i64,
        provider: &str,
        amount: i64,
        currency: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO product_provider_prices (product_id, provider, amount, currency, updated_at)
             VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)
             ON CONFLICT (product_id, provider)
             DO UPDATE SET amount = EXCLUDED.amount, currency = EXCLUDED.currency, updated_at = CURRENT_TIMESTAMP",
        )
        .bind(product_id)
        .bind(provider)
        .bind(amount)
        .bind(currency.trim().to_uppercase())
        .execute(&self.pool)
        .await
        .context("Failed to upsert product override")?;
        Ok(())
    }

    /// Remove a product override (reverts to base USD price).
    pub async fn delete_product_override(&self, product_id: i64, provider: &str) -> Result<()> {
        sqlx::query("DELETE FROM product_provider_prices WHERE product_id = $1 AND provider = $2")
            .bind(product_id)
            .bind(provider)
            .execute(&self.pool)
            .await
            .context("Failed to delete product override")?;
        Ok(())
    }

    pub async fn get_active_plans(&self) -> Result<Vec<Plan>> {
        let mut plans = match sqlx::query_as::<_, Plan>(
            "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial,
             COALESCE(daily_traffic_mb, 0) AS daily_traffic_mb, COALESCE(is_free, FALSE) AS is_free
             FROM plans WHERE is_active = TRUE",
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(plans) => plans,
            Err(primary_err) => {
                // Legacy fallback: older schemas may not have plans.is_trial / is_free / daily_traffic_mb.
                warn!(
                    "Primary active plans query failed (trying legacy fallback): {}",
                    primary_err
                );
                sqlx::query_as::<_, Plan>(
                    "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb,
                     FALSE as is_trial, 0 as daily_traffic_mb, FALSE as is_free
                     FROM plans WHERE is_active = TRUE",
                )
                .fetch_all(&self.pool)
                .await
                .context("Failed to fetch active plans (legacy fallback)")?
            }
        };

        if plans.is_empty() {
            return Ok(Vec::new());
        }

        let plan_ids: Vec<i64> = plans.iter().map(|p| p.id).collect();
        let mut base_prices: HashMap<i64, i64> = HashMap::new();
        let price_rows =
            sqlx::query("SELECT id, price::bigint AS price FROM plans WHERE id = ANY($1)")
                .bind(&plan_ids)
                .fetch_all(&self.pool)
                .await
                .unwrap_or_default();
        for row in price_rows {
            let id = row.try_get::<i64, _>("id").unwrap_or_default();
            let price = row.try_get::<i64, _>("price").unwrap_or_default();
            base_prices.insert(id, price);
        }

        let durations = match sqlx::query_as::<_, PlanDuration>(
            "SELECT id, plan_id, duration_days, price::bigint AS price, created_at FROM plan_durations WHERE plan_id = ANY($1) ORDER BY duration_days ASC",
        )
        .bind(&plan_ids)
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                warn!("Failed to fetch plan_durations (legacy schema?): {}", err);

                if err
                    .to_string()
                    .to_lowercase()
                    .contains("relation \"plan_durations\" does not exist")
                {
                    let _ = sqlx::query(
                        r#"
                        CREATE TABLE IF NOT EXISTS plan_durations (
                            id BIGSERIAL PRIMARY KEY,
                            plan_id BIGINT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
                            duration_days INTEGER NOT NULL,
                            price BIGINT NOT NULL,
                            created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
                        )
                        "#,
                    )
                    .execute(&self.pool)
                    .await;
                }

                Vec::new()
            }
        };

        // Здесь раньше стояло «самовосстановление»: у плана нет строк цены —
        // вставим ему 30 дней по plans.price. Оно удалено, и это не уборка,
        // а исправление.
        //
        // Во-первых, чтение витрины писало в БД. GET, который делает INSERT,
        // удивителен сам по себе, но хуже другое: он отменял решение
        // администратора. План без строк цены — это теперь осмысленное
        // состояние «не продаётся», и ровно так его рисует интерфейс
        // (PurchaseFlow: durations.length === 0 → карточка без кнопок).
        // Убрав у тарифа цену, оператор увидел бы её снова после первого же
        // открытия витрины, без единого следа о том, кто её вернул.
        //
        // Во-вторых, для бесплатного плана оно фабриковало строку «30 дней за
        // $0» — то есть кнопку «купить» на том, что и так выдаётся даром.
        //
        // Легаси-план, у которого цена лежит только в plans.price, теперь
        // показывается как непокупаемый, пока оператор не заведёт срок руками.
        // Это честнее выдуманного тарифа: неверная цена хуже отсутствующей.

        // Характеристики, выводимые из инфраструктуры: сколько серверов и в
        // каких странах даёт этот план. Один запрос на все планы, а не по
        // одному на карточку — витрина открывается на каждый заход в магазин.
        //
        // Считаем только ВКЛЮЧЁННЫЕ и активные узлы: обещать сервер, которого
        // сейчас нет, — то же самое враньё, что и текст в описании, только
        // выглядит достовернее.
        let mut infra: HashMap<i64, (i64, Vec<String>)> = HashMap::new();
        let infra_rows = sqlx::query(
            r#"
            SELECT pg.plan_id,
                   COUNT(DISTINCT m.node_id) AS servers,
                   COALESCE(
                       ARRAY_AGG(DISTINCT n.country_code)
                           FILTER (WHERE n.country_code IS NOT NULL AND n.country_code <> ''),
                       '{}'
                   ) AS countries
            FROM plan_groups pg
            JOIN node_group_members m ON m.group_id = pg.group_id
            JOIN nodes n ON n.id = m.node_id
                        AND n.is_enabled = TRUE
                        AND n.status = 'active'
            WHERE pg.plan_id = ANY($1)
            GROUP BY pg.plan_id
            "#,
        )
        .bind(&plan_ids)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for row in infra_rows {
            let plan_id = row.try_get::<i64, _>("plan_id").unwrap_or_default();
            let servers = row.try_get::<i64, _>("servers").unwrap_or_default();
            let countries = row
                .try_get::<Vec<String>, _>("countries")
                .unwrap_or_default();
            infra.insert(plan_id, (servers, countries));
        }

        for plan in &mut plans {
            plan.durations = durations
                .iter()
                .filter(|d| d.plan_id == plan.id)
                .cloned()
                .collect();

            if let Some((servers, countries)) = infra.remove(&plan.id) {
                plan.server_count = servers;
                plan.countries = countries;
            }
        }

        Ok(plans)
    }

    pub async fn get_plan_duration_by_id(&self, duration_id: i64) -> Result<Option<PlanDuration>> {
        let duration =
            sqlx::query_as::<_, PlanDuration>("SELECT * FROM plan_durations WHERE id = $1")
                .bind(duration_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(duration)
    }

    pub async fn get_plans_admin(&self) -> Result<Vec<Plan>> {
        let mut plans = sqlx::query_as::<_, Plan>(
            "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial,
             COALESCE(daily_traffic_mb, 0) AS daily_traffic_mb, COALESCE(is_free, FALSE) AS is_free
             FROM plans WHERE is_active = TRUE"
        ).fetch_all(&self.pool).await?;
        for plan in &mut plans {
            plan.durations = sqlx::query_as::<_, PlanDuration>(
                "SELECT * FROM plan_durations WHERE plan_id = $1 ORDER BY duration_days ASC",
            )
            .bind(plan.id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        }
        Ok(plans)
    }

    pub async fn get_plan_by_id(&self, id: i64) -> Result<Option<Plan>> {
        let plan_opt = sqlx::query_as::<_, Plan>(
            "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial,
             COALESCE(daily_traffic_mb, 0) AS daily_traffic_mb, COALESCE(is_free, FALSE) AS is_free
             FROM plans WHERE id = $1"
        ).bind(id).fetch_optional(&self.pool).await?;
        if let Some(mut plan) = plan_opt {
            plan.durations = sqlx::query_as::<_, PlanDuration>(
                "SELECT * FROM plan_durations WHERE plan_id = $1 ORDER BY duration_days ASC",
            )
            .bind(plan.id)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
            Ok(Some(plan))
        } else {
            Ok(None)
        }
    }

    pub async fn get_plan_group_ids(&self, plan_id: i64) -> Result<Vec<i64>> {
        let ids: Vec<i64> =
            sqlx::query_scalar("SELECT group_id FROM plan_groups WHERE plan_id = $1")
                .bind(plan_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(ids)
    }

    pub async fn create_plan(
        &self,
        name: &str,
        description: &str,
        device_limit: i32,
        traffic_limit_gb: i32,
        duration_days: Vec<i32>,
        prices: Vec<i64>,
        group_ids: Vec<i64>,
    ) -> Result<i64> {
        self.create_plan_full(
            name,
            description,
            device_limit,
            traffic_limit_gb,
            0,
            false,
            duration_days,
            prices,
            group_ids,
        )
        .await
    }

    pub async fn create_plan_full(
        &self,
        name: &str,
        description: &str,
        device_limit: i32,
        traffic_limit_gb: i32,
        daily_traffic_mb: i32,
        is_free: bool,
        duration_days: Vec<i32>,
        prices: Vec<i64>,
        group_ids: Vec<i64>,
    ) -> Result<i64> {
        let mut tx = self.pool.begin().await?;
        // Keep legacy plans.price in sync with the cheapest active duration.
        let base_price = prices.iter().copied().min().unwrap_or(0);
        let plan_id: i64 = sqlx::query_scalar(
            "INSERT INTO plans (name, description, is_active, traffic_limit_gb, device_limit, price, daily_traffic_mb, is_free)
             VALUES ($1, $2, TRUE, $3, $4, $5, $6, $7) RETURNING id"
        )
            .bind(name).bind(description).bind(traffic_limit_gb).bind(device_limit)
            .bind(base_price).bind(daily_traffic_mb).bind(is_free)
            .fetch_one(&mut *tx).await?;

        for i in 0..duration_days.len().min(prices.len()) {
            sqlx::query(
                "INSERT INTO plan_durations (plan_id, duration_days, price) VALUES ($1, $2, $3)",
            )
            .bind(plan_id)
            .bind(duration_days[i])
            .bind(prices[i])
            .execute(&mut *tx)
            .await?;
        }
        for group_id in group_ids {
            sqlx::query("INSERT INTO plan_groups (plan_id, group_id) VALUES ($1, $2)")
                .bind(plan_id)
                .bind(group_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        let _ = ActivityService::log(
            &self.pool,
            "Plan Created",
            &format!("Created plan: {}", name),
        )
        .await;
        Ok(plan_id)
    }

    pub async fn update_plan(
        &self,
        id: i64,
        name: &str,
        description: &str,
        device_limit: i32,
        traffic_limit_gb: i32,
        duration_days: Vec<i32>,
        prices: Vec<i64>,
        group_ids: Vec<i64>,
    ) -> Result<()> {
        self.update_plan_full(
            id,
            name,
            description,
            device_limit,
            traffic_limit_gb,
            0,
            false,
            duration_days,
            prices,
            group_ids,
        )
        .await
    }

    pub async fn update_plan_full(
        &self,
        id: i64,
        name: &str,
        description: &str,
        device_limit: i32,
        traffic_limit_gb: i32,
        daily_traffic_mb: i32,
        is_free: bool,
        duration_days: Vec<i32>,
        prices: Vec<i64>,
        group_ids: Vec<i64>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let base_price = prices.iter().copied().min().unwrap_or(0);
        sqlx::query(
            "UPDATE plans SET name = $1, description = $2, device_limit = $3, traffic_limit_gb = $4,
             price = $5, daily_traffic_mb = $6, is_free = $7 WHERE id = $8"
        )
            .bind(name)
            .bind(description)
            .bind(device_limit)
            .bind(traffic_limit_gb)
            .bind(base_price)
            .bind(daily_traffic_mb)
            .bind(is_free)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM plan_durations WHERE plan_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for i in 0..duration_days.len().min(prices.len()) {
            sqlx::query(
                "INSERT INTO plan_durations (plan_id, duration_days, price) VALUES ($1, $2, $3)",
            )
            .bind(id)
            .bind(duration_days[i])
            .bind(prices[i])
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("DELETE FROM plan_groups WHERE plan_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for group_id in group_ids {
            sqlx::query("INSERT INTO plan_groups (plan_id, group_id) VALUES ($1, $2)")
                .bind(id)
                .bind(group_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_categories(&self) -> Result<Vec<StoreCategory>> {
        sqlx::query_as::<_, StoreCategory>(
            "SELECT * FROM categories WHERE is_active = TRUE ORDER BY sort_order ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch categories")
    }

    pub async fn get_products_by_category(&self, category_id: i64) -> Result<Vec<Product>> {
        sqlx::query_as::<_, Product>(
            "SELECT * FROM products WHERE category_id = $1 AND is_active = TRUE",
        )
        .bind(category_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch products")
    }

    pub async fn get_all_products(&self) -> Result<Vec<Product>> {
        sqlx::query_as::<_, Product>("SELECT * FROM products WHERE is_active = TRUE")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all products")
    }

    pub async fn get_product(&self, product_id: i64) -> Result<Product> {
        sqlx::query_as::<_, Product>("SELECT * FROM products WHERE id = $1")
            .bind(product_id)
            .fetch_one(&self.pool)
            .await
            .context("Product not found")
    }

    pub async fn create_category(
        &self,
        name: &str,
        description: Option<&str>,
        sort_order: Option<i32>,
    ) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO categories (name, description, sort_order, is_active) VALUES ($1, $2, $3, TRUE) RETURNING id"
        )
        .bind(name)
        .bind(description)
        .bind(sort_order.unwrap_or(0))
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn delete_category(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_product(
        &self,
        category_id: i64,
        name: &str,
        description: Option<&str>,
        price: i64,
        product_type: &str,
        content: Option<&str>,
    ) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO products (category_id, name, description, price, product_type, content, is_active, created_at) VALUES ($1, $2, $3, $4, $5, $6, TRUE, CURRENT_TIMESTAMP) RETURNING id"
        )
        .bind(category_id)
        .bind(name)
        .bind(description)
        .bind(price)
        .bind(product_type)
        .bind(content)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn delete_product(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM products WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn process_order_payment(&self, order_id: i64) -> Result<()> {
        sqlx::query("UPDATE orders SET status = 'paid', paid_at = $1 WHERE id = $2")
            .bind(Utc::now())
            .bind(order_id)
            .execute(&self.pool)
            .await
            .context("Failed to update order status")?;
        Ok(())
    }

    pub async fn get_user_purchased_products(&self, user_id: i64) -> Result<Vec<Product>> {
        sqlx::query_as::<_, Product>(
            r#"
            SELECT p.* 
            FROM products p
            JOIN order_items oi ON oi.product_id = p.id
            JOIN orders o ON o.id = oi.order_id
            WHERE o.user_id = $1 AND o.status = 'paid'
            ORDER BY o.paid_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user purchased products")
    }

    pub async fn add_to_cart(&self, user_id: i64, product_id: i64, quantity: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO cart_items (user_id, product_id, quantity) 
             VALUES ($1, $2, $3) 
             ON CONFLICT(user_id, product_id) 
             DO UPDATE SET quantity = cart_items.quantity + $4",
        )
        .bind(user_id)
        .bind(product_id)
        .bind(quantity)
        .bind(quantity)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_user_cart(&self, user_id: i64) -> Result<Vec<CartItem>> {
        sqlx::query_as::<_, CartItem>(
            "SELECT c.id, c.user_id, c.product_id, c.quantity, p.name as product_name, p.price 
             FROM cart_items c 
             JOIN products p ON c.product_id = p.id 
             WHERE c.user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user cart")
    }

    pub async fn clear_cart(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM cart_items WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn checkout_cart(&self, user_id: i64) -> Result<i64> {
        let cart = self.get_user_cart(user_id).await?;
        if cart.is_empty() {
            return Err(anyhow::anyhow!("Cart is empty"));
        }

        let total_price: i64 = cart.iter().map(|item| item.price * item.quantity).sum();
        let mut tx = self.pool.begin().await?;

        // Create a PENDING order and snapshot its line items. Payment is collected
        // afterwards through the provider-selection step (POST /payment/invoice).
        // Balance is intentionally NOT deducted here — the chosen provider (including
        // the "balance" wallet provider) performs the charge and fulfillment flips the
        // order to 'paid'. This removes the previous double-charge where checkout
        // deducted balance and the Mini App then billed the user again via a provider.
        let order_id: i64 = sqlx::query_scalar(
            "INSERT INTO orders (user_id, total_amount, status) VALUES ($1, $2, 'pending') RETURNING id",
        )
        .bind(user_id)
        .bind(total_price)
        .fetch_one(&mut *tx)
        .await?;

        for item in cart {
            sqlx::query(
                "INSERT INTO order_items (order_id, product_id, price, quantity) VALUES ($1, $2, $3, $4)",
            )
            .bind(order_id)
            .bind(item.product_id)
            .bind(item.price)
            .bind(item.quantity)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query("DELETE FROM cart_items WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let _ = ActivityService::log_tx(
            &mut *tx,
            Some(user_id),
            "Checkout",
            &format!(
                "Order #{} created (pending). Total: {}",
                order_id, total_price
            ),
        )
        .await;
        tx.commit().await?;
        Ok(order_id)
    }

    pub async fn admin_refund_subscription(&self, sub_id: i64, amount: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let sub: caramba_db::models::store::Subscription =
            sqlx::query_as("SELECT * FROM subscriptions WHERE id = $1")
                .bind(sub_id)
                .fetch_one(&mut *tx)
                .await?;

        sqlx::query("DELETE FROM subscriptions WHERE id = $1")
            .bind(sub_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(sub.user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn delete_plan_and_refund(&self, plan_id: i64) -> Result<(i32, i64)> {
        let mut tx = self.pool.begin().await?;

        let subs = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, user_id FROM subscriptions WHERE plan_id = $1",
        )
        .bind(plan_id)
        .fetch_all(&mut *tx)
        .await?;

        let mut total_refunded = 0;
        let mut users_count = 0;

        for (sub_id, user_id) in subs {
            let price: i64 = sqlx::query_scalar("SELECT pd.price FROM plan_durations pd JOIN subscriptions s ON s.plan_id = pd.plan_id WHERE s.id = $1 LIMIT 1")
                .bind(sub_id)
                .fetch_optional(&mut *tx)
                .await?
                .unwrap_or(0);

            if price > 0 {
                sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                    .bind(price)
                    .bind(user_id)
                    .execute(&mut *tx)
                    .await?;
                total_refunded += price;
                users_count += 1;
            }

            sqlx::query("DELETE FROM subscriptions WHERE id = $1")
                .bind(sub_id)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query("DELETE FROM plan_durations WHERE plan_id = $1")
            .bind(plan_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM plans WHERE id = $1")
            .bind(plan_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        let _ = ActivityService::log(
            &self.pool,
            "Admin Action",
            &format!(
                "Deleted plan {} and refunded {} users (total: {})",
                plan_id, users_count, total_refunded
            ),
        )
        .await;
        Ok((users_count, total_refunded))
    }
}
