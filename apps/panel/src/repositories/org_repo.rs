use sqlx::SqlitePool;
use anyhow::{Context, Result};
use crate::models::orgs::{Organization, OrganizationMember};

#[derive(Clone)]
pub struct OrganizationRepository {
    pool: SqlitePool,
}

impl OrganizationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, name: &str, slug: Option<&str>) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO organizations (name, slug) VALUES (?, ?) RETURNING id"
        )
        .bind(name)
        .bind(slug)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn get_by_id(&self, id: i64) -> Result<Option<Organization>> {
        sqlx::query_as::<_, Organization>("SELECT * FROM organizations WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch organization")
    }

    pub async fn add_member(&self, org_id: i64, user_id: i64, role: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO organization_members (organization_id, user_id, role) VALUES (?, ?, ?)"
        )
        .bind(org_id)
        .bind(user_id)
        .bind(role)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_members(&self, org_id: i64) -> Result<Vec<OrganizationMember>> {
        sqlx::query_as::<_, OrganizationMember>("SELECT * FROM organization_members WHERE organization_id = ?")
            .bind(org_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch organization members")
    }

    pub async fn get_user_organizations(&self, user_id: i64) -> Result<Vec<Organization>> {
        sqlx::query_as::<_, Organization>(
            r#"
            SELECT o.* FROM organizations o
            JOIN organization_members om ON om.organization_id = o.id
            WHERE om.user_id = ?
            "#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch user organizations")
    }

    pub async fn adjust_balance(&self, org_id: i64, amount: i64) -> Result<()> {
        sqlx::query("UPDATE organizations SET balance = balance + ? WHERE id = ?")
            .bind(amount)
            .bind(org_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
