use crate::services::activity_service::ActivityService;
use anyhow::{Context, Result};
use caramba_db::models::store::{CartItem, Plan, PlanDuration, Product, StoreCategory};
use chrono::Utc;
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct CatalogService {
    pool: PgPool,
}

impl CatalogService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get_active_plans(&self) -> Result<Vec<Plan>> {
        let mut plans = sqlx::query_as::<_, Plan>(
            "SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE is_active = TRUE"
        )
        .fetch_all(&self.pool)
        .await?;

        if plans.is_empty() {
            return Ok(Vec::new());
        }

        let plan_ids: Vec<i64> = plans.iter().map(|p| p.id).collect();
        let durations = sqlx::query_as::<_, PlanDuration>(
            "SELECT * FROM plan_durations WHERE plan_id = ANY($1) ORDER BY duration_days ASC",
        )
        .bind(&plan_ids)
        .fetch_all(&self.pool)
        .await?;

        for plan in &mut plans {
            plan.durations = durations
                .iter()
                .filter(|d| d.plan_id == plan.id)
                .cloned()
                .collect();
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
        let mut plans = sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE is_trial = FALSE").fetch_all(&self.pool).await?;
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
        let plan_opt = sqlx::query_as::<_, Plan>("SELECT id, name, description, is_active, created_at, device_limit, traffic_limit_gb, is_trial FROM plans WHERE id = $1").bind(id).fetch_optional(&self.pool).await?;
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
        let mut tx = self.pool.begin().await?;
        // Keep legacy plans.price in sync with the cheapest active duration.
        let base_price = prices.iter().copied().min().unwrap_or(0);
        let plan_id: i64 = sqlx::query_scalar("INSERT INTO plans (name, description, is_active, traffic_limit_gb, device_limit, price) VALUES ($1, $2, TRUE, $3, $4, $5) RETURNING id")
            .bind(name).bind(description).bind(traffic_limit_gb).bind(device_limit).bind(base_price).fetch_one(&mut *tx).await?;

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
        let mut tx = self.pool.begin().await?;
        let base_price = prices.iter().copied().min().unwrap_or(0);
        sqlx::query("UPDATE plans SET name = $1, description = $2, device_limit = $3, traffic_limit_gb = $4, price = $5 WHERE id = $6")
            .bind(name)
            .bind(description)
            .bind(device_limit)
            .bind(traffic_limit_gb)
            .bind(base_price)
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

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        if balance < total_price {
            return Err(anyhow::anyhow!(
                "Insufficient balance. Need {}, have {}",
                total_price,
                balance
            ));
        }

        sqlx::query("UPDATE users SET balance = balance - $1 WHERE id = $2")
            .bind(total_price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let order_id: i64 = sqlx::query_scalar("INSERT INTO orders (user_id, total_amount, status, paid_at) VALUES ($1, $2, 'paid', $3) RETURNING id")
            .bind(user_id)
            .bind(total_price)
            .bind(Utc::now())
            .fetch_one(&mut *tx)
            .await?;

        for item in cart {
            sqlx::query(
                "INSERT INTO order_items (order_id, product_id, price) VALUES ($1, $2, $3)",
            )
            .bind(order_id)
            .bind(item.product_id)
            .bind(item.price)
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
            &format!("Checkout complete. Total: {}", total_price),
        )
        .await;
        tx.commit().await?;
        Ok(order_id)
    }

    pub async fn is_trial_plan(&self, id: i64) -> Result<bool> {
        let is_trial: bool = sqlx::query_scalar("SELECT is_trial FROM plans WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        Ok(is_trial)
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

    pub async fn update_trial_plan_limits(
        &self,
        device_limit: i32,
        traffic_limit_gb: i32,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE plans SET device_limit = $1, traffic_limit_gb = $2 WHERE is_trial = TRUE",
        )
        .bind(device_limit)
        .bind(traffic_limit_gb)
        .execute(&self.pool)
        .await?;
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
