use crate::models::groups::{InboundTemplate, NodeGroup, NodeGroupMember, PlanGroup};
use crate::models::network::Inbound;
use crate::models::node::Node;
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::{PgPool, Row, postgres::PgRow};

#[derive(Debug, Clone)]
pub struct NodeRepository {
    pool: PgPool,
}

impl NodeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn is_undefined_table_or_column(err: &sqlx::Error) -> bool {
        match err {
            sqlx::Error::Database(db_err) => {
                matches!(db_err.code().as_deref(), Some("42703") | Some("42P01"))
            }
            _ => false,
        }
    }

    fn row_to_node(row: &PgRow) -> Node {
        Node {
            id: row.try_get::<i64, _>("id").unwrap_or_default(),
            name: row
                .try_get::<String, _>("name")
                .unwrap_or_else(|_| "Unknown Node".to_string()),
            ip: row
                .try_get::<String, _>("ip")
                .unwrap_or_else(|_| "0.0.0.0".to_string()),
            status: row
                .try_get::<String, _>("status")
                .unwrap_or_else(|_| "new".to_string()),
            reality_pub: row
                .try_get::<Option<String>, _>("reality_pub")
                .ok()
                .flatten(),
            reality_priv: row
                .try_get::<Option<String>, _>("reality_priv")
                .ok()
                .flatten(),
            short_id: row.try_get::<Option<String>, _>("short_id").ok().flatten(),
            domain: row.try_get::<Option<String>, _>("domain").ok().flatten(),
            root_password: row
                .try_get::<Option<String>, _>("root_password")
                .ok()
                .flatten(),
            vpn_port: row
                .try_get::<i64, _>("vpn_port")
                .or_else(|_| row.try_get::<i32, _>("vpn_port").map(|v| v as i64))
                .unwrap_or(443),
            last_seen: row
                .try_get::<Option<DateTime<Utc>>, _>("last_seen")
                .ok()
                .flatten(),
            created_at: row
                .try_get::<DateTime<Utc>, _>("created_at")
                .unwrap_or_else(|_| Utc::now()),
            join_token: row
                .try_get::<Option<String>, _>("join_token")
                .ok()
                .flatten(),
            auto_configure: row.try_get::<bool, _>("auto_configure").unwrap_or(false),
            is_enabled: row.try_get::<bool, _>("is_enabled").unwrap_or(true),
            country_code: row
                .try_get::<Option<String>, _>("country_code")
                .ok()
                .flatten(),
            country: row.try_get::<Option<String>, _>("country").ok().flatten(),
            city: row.try_get::<Option<String>, _>("city").ok().flatten(),
            flag: row.try_get::<Option<String>, _>("flag").ok().flatten(),
            reality_sni: row
                .try_get::<Option<String>, _>("reality_sni")
                .ok()
                .flatten(),
            load_stats: row
                .try_get::<Option<String>, _>("load_stats")
                .ok()
                .flatten(),
            check_stats_json: row
                .try_get::<Option<String>, _>("check_stats_json")
                .ok()
                .flatten(),
            sort_order: row.try_get::<i32, _>("sort_order").unwrap_or_default(),
            latitude: row.try_get::<Option<f64>, _>("latitude").ok().flatten(),
            longitude: row.try_get::<Option<f64>, _>("longitude").ok().flatten(),
            config_qos_enabled: row
                .try_get::<bool, _>("config_qos_enabled")
                .unwrap_or(false),
            config_block_torrent: row
                .try_get::<bool, _>("config_block_torrent")
                .unwrap_or(false),
            config_block_ads: row.try_get::<bool, _>("config_block_ads").unwrap_or(false),
            config_block_porn: row.try_get::<bool, _>("config_block_porn").unwrap_or(false),
            last_latency: row.try_get::<Option<f64>, _>("last_latency").ok().flatten(),
            last_cpu: row.try_get::<Option<f64>, _>("last_cpu").ok().flatten(),
            last_ram: row.try_get::<Option<f64>, _>("last_ram").ok().flatten(),
            max_ram: row
                .try_get::<i64, _>("max_ram")
                .or_else(|_| row.try_get::<i32, _>("max_ram").map(|v| v as i64))
                .unwrap_or_default(),
            cpu_cores: row
                .try_get::<i32, _>("cpu_cores")
                .or_else(|_| row.try_get::<i16, _>("cpu_cores").map(i32::from))
                .unwrap_or_default(),
            cpu_model: row.try_get::<Option<String>, _>("cpu_model").ok().flatten(),
            speed_limit_mbps: row
                .try_get::<i32, _>("speed_limit_mbps")
                .unwrap_or_default(),
            max_users: row.try_get::<i32, _>("max_users").unwrap_or_default(),
            current_speed_mbps: row
                .try_get::<i32, _>("current_speed_mbps")
                .unwrap_or_default(),
            relay_id: row.try_get::<Option<i64>, _>("relay_id").ok().flatten(),
            active_connections: row
                .try_get::<Option<i32>, _>("active_connections")
                .ok()
                .flatten(),
            total_ingress: row
                .try_get::<i64, _>("total_ingress")
                .or_else(|_| row.try_get::<i32, _>("total_ingress").map(|v| v as i64))
                .unwrap_or_default(),
            total_egress: row
                .try_get::<i64, _>("total_egress")
                .or_else(|_| row.try_get::<i32, _>("total_egress").map(|v| v as i64))
                .unwrap_or_default(),
            uptime: row
                .try_get::<i64, _>("uptime")
                .or_else(|_| row.try_get::<i32, _>("uptime").map(|v| v as i64))
                .unwrap_or_default(),
            last_session_ingress: row
                .try_get::<i64, _>("last_session_ingress")
                .or_else(|_| {
                    row.try_get::<i32, _>("last_session_ingress")
                        .map(|v| v as i64)
                })
                .unwrap_or_default(),
            last_session_egress: row
                .try_get::<i64, _>("last_session_egress")
                .or_else(|_| {
                    row.try_get::<i32, _>("last_session_egress")
                        .map(|v| v as i64)
                })
                .unwrap_or_default(),
            doomsday_password: row
                .try_get::<Option<String>, _>("doomsday_password")
                .ok()
                .flatten(),
            version: row.try_get::<Option<String>, _>("version").ok().flatten(),
            target_version: row
                .try_get::<Option<String>, _>("target_version")
                .ok()
                .flatten(),
            last_synced_at: row
                .try_get::<Option<DateTime<Utc>>, _>("last_synced_at")
                .ok()
                .flatten(),
            last_sync_trigger: row
                .try_get::<Option<String>, _>("last_sync_trigger")
                .ok()
                .flatten(),
            is_relay: row.try_get::<bool, _>("is_relay").unwrap_or(false),
            pending_log_collection: row
                .try_get::<bool, _>("pending_log_collection")
                .unwrap_or(false),
        }
    }

    fn row_to_inbound(row: &PgRow) -> Inbound {
        Inbound {
            id: row.try_get::<i64, _>("id").unwrap_or_default(),
            node_id: row.try_get::<i64, _>("node_id").unwrap_or_default(),
            tag: row
                .try_get::<String, _>("tag")
                .unwrap_or_else(|_| "inbound".to_string()),
            protocol: row
                .try_get::<String, _>("protocol")
                .unwrap_or_else(|_| "vless".to_string()),
            listen_port: row
                .try_get::<i64, _>("listen_port")
                .or_else(|_| row.try_get::<i32, _>("listen_port").map(|v| v as i64))
                .unwrap_or(443),
            listen_ip: row
                .try_get::<String, _>("listen_ip")
                .unwrap_or_else(|_| "::".to_string()),
            settings: row
                .try_get::<String, _>("settings")
                .unwrap_or_else(|_| "{}".to_string()),
            stream_settings: row
                .try_get::<String, _>("stream_settings")
                .unwrap_or_else(|_| "{}".to_string()),
            remark: row.try_get::<Option<String>, _>("remark").ok().flatten(),
            enable: row.try_get::<bool, _>("enable").unwrap_or(true),
            renew_interval_mins: row
                .try_get::<i64, _>("renew_interval_mins")
                .or_else(|_| {
                    row.try_get::<i32, _>("renew_interval_mins")
                        .map(|v| v as i64)
                })
                .unwrap_or(0),
            port_range_start: row
                .try_get::<i64, _>("port_range_start")
                .or_else(|_| row.try_get::<i32, _>("port_range_start").map(|v| v as i64))
                .unwrap_or(10000),
            port_range_end: row
                .try_get::<i64, _>("port_range_end")
                .or_else(|_| row.try_get::<i32, _>("port_range_end").map(|v| v as i64))
                .unwrap_or(60000),
            last_rotated_at: row
                .try_get::<Option<DateTime<Utc>>, _>("last_rotated_at")
                .ok()
                .flatten(),
            created_at: row
                .try_get::<Option<DateTime<Utc>>, _>("created_at")
                .ok()
                .flatten(),
        }
    }

    fn parse_datetime_utc(raw: &str) -> DateTime<Utc> {
        if let Ok(ts) = raw.parse::<i64>() {
            if let Some(dt) = DateTime::<Utc>::from_timestamp(ts, 0) {
                return dt;
            }
        }

        if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
            return dt.with_timezone(&Utc);
        }

        for fmt in [
            "%Y-%m-%d %H:%M:%S%.f%:z",
            "%Y-%m-%d %H:%M:%S%.f%#z",
            "%Y-%m-%d %H:%M:%S%:z",
            "%Y-%m-%d %H:%M:%S%#z",
        ] {
            if let Ok(dt) = DateTime::parse_from_str(raw, fmt) {
                return dt.with_timezone(&Utc);
            }
        }

        for fmt in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%d %H:%M:%S"] {
            if let Ok(dt) = NaiveDateTime::parse_from_str(raw, fmt) {
                return DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc);
            }
        }

        Utc::now()
    }

    fn row_to_inbound_template(row: &PgRow) -> InboundTemplate {
        let created_at = row
            .try_get::<DateTime<Utc>, _>("created_at")
            .or_else(|_| {
                row.try_get::<NaiveDateTime, _>("created_at")
                    .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
            })
            .or_else(|_| {
                row.try_get::<String, _>("created_at")
                    .map(|raw| Self::parse_datetime_utc(&raw))
            })
            .unwrap_or_else(|_| Utc::now());

        let target_group_id = row
            .try_get::<Option<i64>, _>("target_group_id")
            .ok()
            .flatten()
            .or_else(|| {
                row.try_get::<Option<i32>, _>("target_group_id")
                    .ok()
                    .flatten()
                    .map(|v| v as i64)
            });

        let renew_hours = row
            .try_get::<i64, _>("renew_interval_hours")
            .or_else(|_| {
                row.try_get::<i32, _>("renew_interval_hours")
                    .map(|v| v as i64)
            })
            .unwrap_or(0);
        let renew_mins = row
            .try_get::<i64, _>("renew_interval_mins")
            .or_else(|_| {
                row.try_get::<i32, _>("renew_interval_mins")
                    .map(|v| v as i64)
            })
            .unwrap_or(0);
        let is_active = row
            .try_get::<bool, _>("is_active")
            .or_else(|_| row.try_get::<i16, _>("is_active").map(|v| v != 0))
            .or_else(|_| row.try_get::<i32, _>("is_active").map(|v| v != 0))
            .unwrap_or(true);

        InboundTemplate {
            id: row.try_get::<i64, _>("id").unwrap_or_default(),
            name: row
                .try_get::<String, _>("name")
                .unwrap_or_else(|_| "Unnamed Template".to_string()),
            protocol: row
                .try_get::<String, _>("protocol")
                .unwrap_or_else(|_| "vless".to_string()),
            settings_template: row
                .try_get::<String, _>("settings_template")
                .unwrap_or_else(|_| "{}".to_string()),
            stream_settings_template: row
                .try_get::<String, _>("stream_settings_template")
                .unwrap_or_else(|_| "{}".to_string()),
            target_group_id,
            port_range_start: row
                .try_get::<i64, _>("port_range_start")
                .or_else(|_| row.try_get::<i32, _>("port_range_start").map(|v| v as i64))
                .unwrap_or(10000),
            port_range_end: row
                .try_get::<i64, _>("port_range_end")
                .or_else(|_| row.try_get::<i32, _>("port_range_end").map(|v| v as i64))
                .unwrap_or(60000),
            renew_interval_hours: renew_hours,
            renew_interval_mins: renew_mins,
            is_active,
            created_at,
        }
    }

    // ==================== NODES ====================

    pub async fn get_all_nodes(&self) -> Result<Vec<Node>> {
        let rows = sqlx::query("SELECT * FROM nodes ORDER BY sort_order ASC, name ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all nodes")?;
        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_node(&row))
            .collect())
    }

    pub async fn get_active_nodes(&self) -> Result<Vec<Node>> {
        let rows = sqlx::query(
            "SELECT * FROM nodes WHERE status = 'active' ORDER BY sort_order ASC, name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch active nodes")?;
        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_node(&row))
            .collect())
    }

    pub async fn get_node_by_id(&self, id: i64) -> Result<Option<Node>> {
        let row = sqlx::query("SELECT * FROM nodes WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch node by ID")?;
        Ok(row.map(|r| Self::row_to_node(&r)))
    }

    pub async fn get_active_node_ids(&self) -> Result<Vec<i64>> {
        sqlx::query_scalar("SELECT id FROM nodes WHERE status = 'active'")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch active node IDs")
    }

    pub async fn get_relay_clients(&self, node_id: i64) -> Result<Vec<Node>> {
        let rows = sqlx::query("SELECT * FROM nodes WHERE relay_id = $1 AND status = 'active'")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch relay clients")?;
        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_node(&row))
            .collect())
    }

    pub async fn create_node(&self, node: &Node) -> Result<i64> {
        let primary_insert = sqlx::query_scalar(
            r#"
            INSERT INTO nodes (
                name, ip, domain, country, city, flag, 
                status, load_stats, check_stats_json, sort_order,
                join_token, vpn_port, auto_configure, is_enabled,
                reality_pub, reality_priv, short_id, reality_sni,
                relay_id, doomsday_password
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
            RETURNING id
            "#
        )
        .bind(&node.name)
        .bind(&node.ip)
        .bind(&node.domain)
        .bind(&node.country)
        .bind(&node.city)
        .bind(&node.flag)
        .bind(&node.status)
        .bind(&node.load_stats)
        .bind(&node.check_stats_json)
        .bind(node.sort_order)
        .bind(&node.join_token)
        .bind(node.vpn_port)
        .bind(node.auto_configure)
        .bind(node.is_enabled)
        .bind(&node.reality_pub)
        .bind(&node.reality_priv)
        .bind(&node.short_id)
        .bind(&node.reality_sni)
        .bind(node.relay_id)
        .bind(&node.doomsday_password)
        .fetch_one(&self.pool)
        .await;

        match primary_insert {
            Ok(id) => Ok(id),
            Err(e) => {
                // Backward-compat for installations where nodes.relay_id is not migrated yet.
                let msg = e.to_string();
                if msg.contains("does not exist") {
                    let secondary = sqlx::query_scalar(
                        r#"
                        INSERT INTO nodes (
                            name, ip, domain, country, city, flag,
                            status, load_stats, check_stats_json, sort_order,
                            join_token, vpn_port, auto_configure, is_enabled,
                            reality_pub, reality_priv, short_id, reality_sni,
                            doomsday_password
                        )
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
                        RETURNING id
                        "#
                    )
                    .bind(&node.name)
                    .bind(&node.ip)
                    .bind(&node.domain)
                    .bind(&node.country)
                    .bind(&node.city)
                    .bind(&node.flag)
                    .bind(&node.status)
                    .bind(&node.load_stats)
                    .bind(&node.check_stats_json)
                    .bind(node.sort_order)
                    .bind(&node.join_token)
                    .bind(node.vpn_port)
                    .bind(node.auto_configure)
                    .bind(node.is_enabled)
                    .bind(&node.reality_pub)
                    .bind(&node.reality_priv)
                    .bind(&node.short_id)
                    .bind(&node.reality_sni)
                    .bind(&node.doomsday_password)
                    .fetch_one(&self.pool)
                    .await;

                    match secondary {
                        Ok(id) => Ok(id),
                        Err(e2) => {
                            let msg2 = e2.to_string();
                            if msg2.contains("does not exist") {
                                let id = sqlx::query_scalar(
                                    r#"
                                    INSERT INTO nodes (name, ip, status, join_token, vpn_port)
                                    VALUES ($1, $2, $3, $4, $5)
                                    RETURNING id
                                    "#,
                                )
                                .bind(&node.name)
                                .bind(&node.ip)
                                .bind(&node.status)
                                .bind(&node.join_token)
                                .bind(node.vpn_port)
                                .fetch_one(&self.pool)
                                .await?;
                                Ok(id)
                            } else {
                                Err(e2.into())
                            }
                        }
                    }
                } else {
                    Err(e.into())
                }
            }
        }
    }

    pub async fn update_node(&self, node: &Node) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE nodes 
            SET name=$1, ip=$2, domain=$3, country=$4, city=$5, flag=$6, status=$7, load_stats=$8, check_stats_json=$9, sort_order=$10,
                join_token=$11, vpn_port=$12, auto_configure=$13, is_enabled=$14, 
                reality_pub=$15, reality_priv=$16, short_id=$17, reality_sni=$18, 
                relay_id=$19, doomsday_password=$20
            WHERE id=$21
            "#
        )
        .bind(&node.name)
        .bind(&node.ip)
        .bind(&node.domain)
        .bind(&node.country)
        .bind(&node.city)
        .bind(&node.flag)
        .bind(&node.status)
        .bind(&node.load_stats)
        .bind(&node.check_stats_json)
        .bind(node.sort_order)
        .bind(&node.join_token)
        .bind(node.vpn_port)
        .bind(node.auto_configure)
        .bind(node.is_enabled)
        .bind(&node.reality_pub)
        .bind(&node.reality_priv)
        .bind(&node.short_id)
        .bind(&node.reality_sni)
        .bind(node.relay_id)
        .bind(&node.doomsday_password)
        .bind(node.id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ==================== INBOUNDS ====================

    pub async fn get_inbounds_by_node(&self, node_id: i64) -> Result<Vec<Inbound>> {
        let rows = sqlx::query("SELECT * FROM inbounds WHERE node_id = $1")
            .bind(node_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch inbounds for node")?;
        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_inbound(&row))
            .collect())
    }

    pub async fn get_all_inbounds(&self) -> Result<Vec<Inbound>> {
        let rows = sqlx::query("SELECT * FROM inbounds")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch all inbounds")?;
        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_inbound(&row))
            .collect())
    }

    pub async fn upsert_inbound(&self, inbound: &Inbound) -> Result<()> {
        let primary = sqlx::query(
            r#"
            INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings, enable, listen_ip, remark)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT(node_id, listen_port) DO UPDATE SET
                tag=excluded.tag,
                protocol=excluded.protocol,
                settings=excluded.settings,
                stream_settings=excluded.stream_settings,
                enable=excluded.enable,
                listen_ip=excluded.listen_ip,
                remark=excluded.remark
            "#
        )
        .bind(inbound.node_id)
        .bind(&inbound.tag)
        .bind(&inbound.protocol)
        .bind(inbound.listen_port)
        .bind(&inbound.settings)
        .bind(&inbound.stream_settings)
        .bind(inbound.enable)
        .bind(&inbound.listen_ip)
        .bind(&inbound.remark)
        .execute(&self.pool)
        .await;

        match primary {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();

                // Compatibility fallback for legacy schemas where ON CONFLICT pair or some columns are missing.
                if msg.contains("no unique or exclusion constraint")
                    || msg.contains("listen_ip")
                    || msg.contains("remark")
                    || msg.contains("enable")
                    || msg.contains("does not exist")
                {
                    // Try update existing row by node+tag first.
                    let _ = sqlx::query(
                        "UPDATE inbounds SET protocol = $1, listen_port = $2, settings = $3, stream_settings = $4 WHERE node_id = $5 AND tag = $6",
                    )
                    .bind(&inbound.protocol)
                    .bind(inbound.listen_port)
                    .bind(&inbound.settings)
                    .bind(&inbound.stream_settings)
                    .bind(inbound.node_id)
                    .bind(&inbound.tag)
                    .execute(&self.pool)
                    .await;

                    // Insert if no row exists for this tag.
                    sqlx::query(
                        "INSERT INTO inbounds (node_id, tag, protocol, listen_port, settings, stream_settings) SELECT $1, $2, $3, $4, $5, $6 WHERE NOT EXISTS (SELECT 1 FROM inbounds WHERE node_id = $1 AND tag = $2)",
                    )
                    .bind(inbound.node_id)
                    .bind(&inbound.tag)
                    .bind(&inbound.protocol)
                    .bind(inbound.listen_port)
                    .bind(&inbound.settings)
                    .bind(&inbound.stream_settings)
                    .execute(&self.pool)
                    .await?;

                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    pub async fn get_inbound_by_id(&self, id: i64) -> Result<Option<Inbound>> {
        let row = sqlx::query("SELECT * FROM inbounds WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to fetch inbound by ID")?;
        Ok(row.map(|r| Self::row_to_inbound(&r)))
    }

    pub async fn delete_inbound_by_id(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM inbounds WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_all_inbound_templates(&self) -> Result<Vec<InboundTemplate>> {
        let primary = sqlx::query(
            r#"
            SELECT
                id,
                name,
                protocol,
                settings_template,
                stream_settings_template,
                target_group_id,
                port_range_start,
                port_range_end,
                renew_interval_hours,
                renew_interval_mins,
                is_active,
                created_at
            FROM inbound_templates
            WHERE COALESCE(is_active, TRUE) = TRUE
            ORDER BY name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await;

        match primary {
            Ok(rows) => Ok(rows
                .into_iter()
                .map(|row| Self::row_to_inbound_template(&row))
                .collect()),
            Err(e) if Self::is_undefined_table_or_column(&e) => {
                // Legacy fallback where rotation/activity columns might not exist.
                let rows = sqlx::query(
                    r#"
                    SELECT
                        id,
                        name,
                        protocol,
                        settings_template,
                        stream_settings_template,
                        target_group_id,
                        10000::BIGINT AS port_range_start,
                        60000::BIGINT AS port_range_end,
                        0::BIGINT AS renew_interval_hours,
                        0::BIGINT AS renew_interval_mins,
                        TRUE AS is_active,
                        created_at
                    FROM inbound_templates
                    ORDER BY name ASC
                    "#,
                )
                .fetch_all(&self.pool)
                .await
                .context("Failed to fetch all inbound templates")?;
                Ok(rows
                    .into_iter()
                    .map(|row| Self::row_to_inbound_template(&row))
                    .collect())
            }
            Err(e) => Err(e).context("Failed to fetch all inbound templates"),
        }
    }

    pub async fn get_inbound_template_by_id(&self, id: i64) -> Result<Option<InboundTemplate>> {
        let primary = sqlx::query(
            r#"
            SELECT
                id,
                name,
                protocol,
                settings_template,
                stream_settings_template,
                target_group_id,
                port_range_start,
                port_range_end,
                renew_interval_hours,
                renew_interval_mins,
                is_active,
                created_at
            FROM inbound_templates
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await;

        match primary {
            Ok(row) => Ok(row.map(|r| Self::row_to_inbound_template(&r))),
            Err(e) if Self::is_undefined_table_or_column(&e) => {
                let row = sqlx::query(
                    r#"
                    SELECT
                        id,
                        name,
                        protocol,
                        settings_template,
                        stream_settings_template,
                        target_group_id,
                        10000::BIGINT AS port_range_start,
                        60000::BIGINT AS port_range_end,
                        0::BIGINT AS renew_interval_hours,
                        0::BIGINT AS renew_interval_mins,
                        TRUE AS is_active,
                        created_at
                    FROM inbound_templates
                    WHERE id = $1
                    "#,
                )
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .context("Failed to fetch inbound template by id")?;
                Ok(row.map(|r| Self::row_to_inbound_template(&r)))
            }
            Err(e) => Err(e).context("Failed to fetch inbound template by id"),
        }
    }

    // ==================== GROUPS (NODES) ====================

    pub async fn get_all_groups(&self) -> Result<Vec<NodeGroup>> {
        sqlx::query_as::<_, NodeGroup>("SELECT * FROM node_groups ORDER BY id ASC")
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch node groups")
    }

    pub async fn get_group_nodes(&self, group_id: i64) -> Result<Vec<i64>> {
        sqlx::query_scalar("SELECT node_id FROM node_group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch group nodes")
    }

    pub async fn get_group_members(&self, group_id: i64) -> Result<Vec<NodeGroupMember>> {
        sqlx::query_as::<_, NodeGroupMember>("SELECT * FROM node_group_members WHERE group_id = $1")
            .bind(group_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch group members")
    }

    pub async fn get_plan_groups(&self, plan_id: i64) -> Result<Vec<PlanGroup>> {
        sqlx::query_as::<_, PlanGroup>("SELECT * FROM plan_groups WHERE plan_id = $1")
            .bind(plan_id)
            .fetch_all(&self.pool)
            .await
            .context("Failed to fetch plan groups")
    }

    pub async fn get_active_nodes_by_groups(&self, group_ids: &[i64]) -> Result<Vec<Node>> {
        if group_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            r#"
            SELECT DISTINCT n.* FROM nodes n
            JOIN node_group_members gn ON gn.node_id = n.id
            WHERE n.status = 'active' AND gn.group_id = ANY($1)
            ORDER BY n.sort_order ASC
            "#,
        )
        .bind(group_ids)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch nodes by groups")?;
        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_node(&row))
            .collect())
    }

    pub async fn create_group(&self, name: &str, description: Option<&str>) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO node_groups (name, description, created_at) VALUES ($1, $2, CURRENT_TIMESTAMP) RETURNING id"
        )
        .bind(name)
        .bind(description)
        .fetch_one(&self.pool)
        .await
        .context("Failed to create node group")?;
        Ok(id)
    }

    pub async fn get_group_by_name(
        &self,
        name: &str,
    ) -> Result<Option<crate::models::groups::NodeGroup>> {
        sqlx::query_as::<_, crate::models::groups::NodeGroup>(
            "SELECT * FROM node_groups WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch group by name")
    }

    pub async fn add_node_to_group(&self, node_id: i64, group_id: i64) -> Result<()> {
        sqlx::query("INSERT INTO node_group_members (node_id, group_id, created_at) VALUES ($1, $2, CURRENT_TIMESTAMP) ON CONFLICT DO NOTHING")
            .bind(node_id)
            .bind(group_id)
            .execute(&self.pool)
            .await
            .context("Failed to add node to group")?;
        Ok(())
    }

    pub async fn get_groups_by_node(
        &self,
        node_id: i64,
    ) -> Result<Vec<crate::models::groups::NodeGroup>> {
        sqlx::query_as::<_, crate::models::groups::NodeGroup>(
            "SELECT g.* FROM node_groups g JOIN node_group_members m ON m.group_id = g.id WHERE m.node_id = $1"
        )
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch node groups")
    }

    // ==================== BUSINESS LOGIC queries ====================

    pub async fn get_nodes_for_plan(&self, plan_id: i64) -> Result<Vec<Node>> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT n.* 
            FROM nodes n
            JOIN node_group_members ngm ON n.id = ngm.node_id
            JOIN plan_groups pg ON ngm.group_id = pg.group_id
            WHERE pg.plan_id = $1 AND n.status = 'active'
            ORDER BY n.sort_order ASC
            "#,
        )
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await?;
        let nodes: Vec<Node> = rows
            .into_iter()
            .map(|row| Self::row_to_node(&row))
            .collect();

        if !nodes.is_empty() {
            return Ok(nodes);
        }

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plan_groups WHERE plan_id = $1")
            .bind(plan_id)
            .fetch_one(&self.pool)
            .await?;

        if count == 0 {
            return self.get_active_nodes().await;
        }

        Ok(Vec::new())
    }

    pub async fn get_inbounds_for_plan(&self, plan_id: i64) -> Result<Vec<Inbound>> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT i.* FROM inbounds i
            LEFT JOIN plan_inbounds pi ON pi.inbound_id = i.id
            LEFT JOIN plan_nodes pn ON pn.node_id = i.node_id
            LEFT JOIN node_group_members ngm ON ngm.node_id = i.node_id
            LEFT JOIN plan_groups pg ON pg.group_id = ngm.group_id
            WHERE (pi.plan_id = $1 OR pn.plan_id = $2 OR pg.plan_id = $3) AND i.enable = TRUE
            "#,
        )
        .bind(plan_id)
        .bind(plan_id)
        .bind(plan_id)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch inbounds for plan")?;
        Ok(rows
            .into_iter()
            .map(|row| Self::row_to_inbound(&row))
            .collect())
    }

    pub async fn delete_node(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_status(&self, id: i64, status: &str) -> Result<()> {
        sqlx::query("UPDATE nodes SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn toggle_enabled(&self, id: i64) -> Result<bool> {
        let current: bool = sqlx::query_scalar("SELECT is_enabled FROM nodes WHERE id = $1")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;

        let new_val = !current;
        sqlx::query("UPDATE nodes SET is_enabled = $1 WHERE id = $2")
            .bind(new_val)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(new_val)
    }

    pub async fn get_linked_plans(&self, node_id: i64, inbound_id: i64) -> Result<Vec<i64>> {
        let plans: Vec<i64> = sqlx::query_scalar(
            r#"
            SELECT plan_id FROM plan_inbounds WHERE inbound_id = $1
            UNION
            SELECT plan_id FROM plan_nodes WHERE node_id = $2
            UNION
            SELECT pg.plan_id 
            FROM plan_groups pg
            JOIN node_group_members ngm ON pg.group_id = ngm.group_id
            WHERE ngm.node_id = $3
            "#,
        )
        .bind(inbound_id)
        .bind(node_id)
        .bind(node_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(plans)
    }

    pub async fn link_inbound_to_plan(&self, plan_id: i64, inbound_id: i64) -> Result<()> {
        sqlx::query("INSERT INTO plan_inbounds (plan_id, inbound_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(plan_id)
            .bind(inbound_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn link_node_inbounds_to_plan(&self, plan_id: i64, node_id: i64) -> Result<()> {
        sqlx::query("INSERT INTO plan_inbounds (plan_id, inbound_id) SELECT $1, id FROM inbounds WHERE node_id = $2 ON CONFLICT DO NOTHING")
            .bind(plan_id)
            .bind(node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_templates_for_group(&self, group_id: i64) -> Result<Vec<InboundTemplate>> {
        let primary = sqlx::query(
            r#"
            SELECT
                id,
                name,
                protocol,
                settings_template,
                stream_settings_template,
                target_group_id,
                port_range_start,
                port_range_end,
                renew_interval_hours,
                renew_interval_mins,
                is_active,
                created_at
            FROM inbound_templates
            WHERE target_group_id = $1
              AND COALESCE(is_active, TRUE) = TRUE
            ORDER BY name ASC
            "#,
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await;

        match primary {
            Ok(rows) => Ok(rows
                .into_iter()
                .map(|row| Self::row_to_inbound_template(&row))
                .collect()),
            Err(e) if Self::is_undefined_table_or_column(&e) => {
                let rows = sqlx::query(
                    r#"
                    SELECT
                        id,
                        name,
                        protocol,
                        settings_template,
                        stream_settings_template,
                        target_group_id,
                        10000::BIGINT AS port_range_start,
                        60000::BIGINT AS port_range_end,
                        0::BIGINT AS renew_interval_hours,
                        0::BIGINT AS renew_interval_mins,
                        TRUE AS is_active,
                        created_at
                    FROM inbound_templates
                    WHERE target_group_id = $1
                    ORDER BY name ASC
                    "#,
                )
                .bind(group_id)
                .fetch_all(&self.pool)
                .await
                .context("Failed to fetch templates for group")?;
                Ok(rows
                    .into_iter()
                    .map(|row| Self::row_to_inbound_template(&row))
                    .collect())
            }
            Err(e) => Err(e).context("Failed to fetch templates for group"),
        }
    }
}
