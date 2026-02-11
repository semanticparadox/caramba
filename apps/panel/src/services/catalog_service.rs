use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::store::{StoreCategory, Product, CartItem};
use chrono::Utc;
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct CatalogService {
    pool: SqlitePool,
}

impl CatalogService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_categories(&self) -> Result<Vec<StoreCategory>> {
        sqlx::query_as::<_, StoreCategory>("SELECT * FROM categories WHERE is_active = 1 ORDER BY sort_order ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch categories")
    }

    pub async fn get_products_by_category(&self, category_id: i64) -> Result<Vec<Product>> {
        sqlx::query_as::<_, Product>("SELECT * FROM products WHERE category_id = ? AND is_active = 1")
            .bind(category_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch products")
    }

    pub async fn get_all_products(&self) -> Result<Vec<Product>> {
         sqlx::query_as::<_, Product>("SELECT * FROM products WHERE is_active = 1")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all products")
    }

    pub async fn get_product(&self, product_id: i64) -> Result<Product> {
        sqlx::query_as::<_, Product>("SELECT * FROM products WHERE id = ?")
            .bind(product_id)
            .fetch_one(&self.pool)
            .await
            .context("Product not found")
    }

    pub async fn create_category(&self, name: &str, description: Option<&str>, sort_order: Option<i32>) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO categories (name, description, sort_order, is_active) VALUES (?, ?, ?, 1) RETURNING id"
        )
        .bind(name)
        .bind(description)
        .bind(sort_order.unwrap_or(0))
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn delete_category(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM categories WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_product(&self, category_id: i64, name: &str, description: Option<&str>, price: i64, product_type: &str, content: Option<&str>) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO products (category_id, name, description, price, product_type, content, is_active, created_at) VALUES (?, ?, ?, ?, ?, ?, 1, CURRENT_TIMESTAMP) RETURNING id"
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
        sqlx::query("DELETE FROM products WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn process_order_payment(&self, order_id: i64) -> Result<()> {
        sqlx::query("UPDATE orders SET status = 'paid', paid_at = ? WHERE id = ?")
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
            WHERE o.user_id = ? AND o.status = 'paid'
            ORDER BY o.paid_at DESC
            "#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user purchased products")
    }

    pub async fn add_to_cart(&self, user_id: i64, product_id: i64, quantity: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO cart_items (user_id, product_id, quantity) 
             VALUES (?, ?, ?) 
             ON CONFLICT(user_id, product_id) 
             DO UPDATE SET quantity = quantity + ?"
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
             WHERE c.user_id = ?"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user cart")
    }

    pub async fn clear_cart(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM cart_items WHERE user_id = ?")
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

        let balance: i64 = sqlx::query_scalar("SELECT balance FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

        if balance < total_price {
            return Err(anyhow::anyhow!("Insufficient balance. Need {}, have {}", total_price, balance));
        }

        sqlx::query("UPDATE users SET balance = balance - ? WHERE id = ?")
            .bind(total_price)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let order_id: i64 = sqlx::query("INSERT INTO orders (user_id, total_amount, status, paid_at) VALUES (?, ?, 'paid', ?) RETURNING id")
            .bind(user_id)
            .bind(total_price)
            .bind(Utc::now())
            .fetch_one(&mut *tx)
            .await?
            .get(0);

        for item in cart {
            sqlx::query("INSERT INTO order_items (order_id, product_id, price) VALUES (?, ?, ?)")
                .bind(order_id)
                .bind(item.product_id)
                .bind(item.price)
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query("DELETE FROM cart_items WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(order_id)
    }
}
