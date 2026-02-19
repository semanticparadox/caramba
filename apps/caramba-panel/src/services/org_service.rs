use anyhow::Result;
use caramba_db::models::orgs::{OrgRole, Organization};
use caramba_db::repositories::org_repo::OrganizationRepository;

#[derive(Clone)]
pub struct OrganizationService {
    org_repo: OrganizationRepository,
}

impl OrganizationService {
    pub fn new(org_repo: OrganizationRepository) -> Self {
        Self { org_repo }
    }

    pub async fn create_organization(
        &self,
        owner_id: i64,
        name: &str,
        slug: Option<&str>,
    ) -> Result<Organization> {
        let org_id = self.org_repo.create(name, slug).await?;
        self.org_repo
            .add_member(org_id, owner_id, &OrgRole::Owner.to_string())
            .await?;

        self.org_repo
            .get_by_id(org_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to retrieve created organization"))
    }

    pub async fn get_user_organizations(&self, user_id: i64) -> Result<Vec<Organization>> {
        self.org_repo.get_user_organizations(user_id).await
    }

    pub async fn add_admin(&self, org_id: i64, user_id: i64) -> Result<()> {
        self.org_repo
            .add_member(org_id, user_id, &OrgRole::Admin.to_string())
            .await
    }

    pub async fn add_member(&self, org_id: i64, user_id: i64) -> Result<()> {
        self.org_repo
            .add_member(org_id, user_id, &OrgRole::Member.to_string())
            .await
    }
}
