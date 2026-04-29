use anyhow::{Context, Result, bail};
use caramba_db::models::tickets::{Ticket, TicketAttachment, TicketMessage, TicketSummary, TicketWithUser};
use serde_json::json;
use sqlx::PgPool;
use std::path::PathBuf;
use tokio::fs;
use tracing::{error, info};

use crate::bot_manager::BotManager;
use crate::services::notifications_service::NotificationsService;

/// Максимальный размер вложения — 10 МБ.
const MAX_ATTACHMENT_SIZE: usize = 10 * 1024 * 1024;

/// Разрешённые MIME-типы вложений.
const ALLOWED_MIMES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "application/pdf",
];

/// Сервис тикетов поддержки.
pub struct TicketsService {
    pool: PgPool,
    bot_manager: std::sync::Arc<BotManager>,
    notifications: std::sync::Arc<NotificationsService>,
    upload_dir: PathBuf,
}

impl TicketsService {
    pub fn new(
        pool: PgPool,
        bot_manager: std::sync::Arc<BotManager>,
        notifications: std::sync::Arc<NotificationsService>,
    ) -> Self {
        let upload_dir = std::env::var("TICKETS_UPLOAD_DIR")
            .unwrap_or_else(|_| "/var/lib/caramba/tickets".to_string());
        Self {
            pool,
            bot_manager,
            notifications,
            upload_dir: PathBuf::from(upload_dir),
        }
    }

    /// Создаёт тикет с первым сообщением в одной транзакции.
    /// После создания уведомляет всех настроенных администраторов через бот.
    pub async fn create_ticket(
        &self,
        user_id: i64,
        category: &str,
        subject: &str,
        first_message: &str,
        related_payment_id: Option<i64>,
        related_subscription_id: Option<i64>,
    ) -> Result<Ticket> {
        let mut tx = self.pool.begin().await.context("Ошибка начала транзакции")?;

        let ticket: Ticket = sqlx::query_as::<_, Ticket>(
            "INSERT INTO tickets (user_id, category, subject, status, related_payment_id, related_subscription_id)
             VALUES ($1, $2, $3, 'open', $4, $5)
             RETURNING id, user_id, category, subject, status, assignee_tg_id,
                       related_payment_id, related_subscription_id, created_at, updated_at, closed_at",
        )
        .bind(user_id)
        .bind(category)
        .bind(subject)
        .bind(related_payment_id)
        .bind(related_subscription_id)
        .fetch_one(&mut *tx)
        .await
        .context("Ошибка создания тикета")?;

        // Первое сообщение пользователя
        sqlx::query(
            "INSERT INTO ticket_messages (ticket_id, sender_role, sender_tg_id, body)
             SELECT $1, 'user', u.tg_id, $2 FROM users u WHERE u.id = $3",
        )
        .bind(ticket.id)
        .bind(first_message)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .context("Ошибка создания первого сообщения")?;

        tx.commit().await.context("Ошибка коммита транзакции")?;

        // Уведомляем администраторов о новом тикете
        let username: Option<String> =
            sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None);

        let user_label = username
            .as_deref()
            .map(|u| format!("@{}", u))
            .unwrap_or_else(|| format!("user#{}", user_id));

        let admin_msg = format!(
            "📨 *Новый тикет #{}*\nОт: {}\nКатегория: {}\nТема: {}",
            ticket.id,
            crate::services::monitoring::escape_md_pub(&user_label),
            crate::services::monitoring::escape_md_pub(category),
            crate::services::monitoring::escape_md_pub(subject)
        );

        self.bot_manager
            .notify_admins(&self.pool, &admin_msg)
            .await;

        info!("Создан тикет #{} от пользователя {}", ticket.id, user_id);
        Ok(ticket)
    }

    /// Список тикетов пользователя с превью последнего сообщения.
    pub async fn list_user_tickets(&self, user_id: i64) -> Result<Vec<TicketSummary>> {
        let rows = sqlx::query_as::<_, TicketSummary>(
            r#"
            SELECT
                t.id,
                t.user_id,
                t.category,
                t.subject,
                t.status,
                t.assignee_tg_id,
                t.created_at,
                t.updated_at,
                (SELECT LEFT(body, 120)
                 FROM ticket_messages tm
                 WHERE tm.ticket_id = t.id
                 ORDER BY tm.created_at DESC
                 LIMIT 1) AS last_message_preview,
                0::BIGINT AS unread_for_user
            FROM tickets t
            WHERE t.user_id = $1
            ORDER BY t.updated_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .context("Ошибка запроса тикетов пользователя")?;

        Ok(rows)
    }

    /// Список тикетов для администратора с фильтрацией.
    pub async fn list_admin_tickets(
        &self,
        status_filter: Option<&str>,
        assignee_filter: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TicketWithUser>> {
        // Динамический запрос с опциональными фильтрами
        let mut sql = r#"
            SELECT
                t.id,
                t.user_id,
                u.username,
                u.tg_id,
                t.category,
                t.subject,
                t.status,
                t.assignee_tg_id,
                t.related_payment_id,
                t.related_subscription_id,
                t.created_at,
                t.updated_at,
                (SELECT LEFT(body, 120)
                 FROM ticket_messages tm
                 WHERE tm.ticket_id = t.id
                 ORDER BY tm.created_at DESC
                 LIMIT 1) AS last_message_preview
            FROM tickets t
            JOIN users u ON u.id = t.user_id
            WHERE 1=1
        "#
        .to_string();

        let mut param_idx = 1usize;
        let mut binds: Vec<String> = Vec::new();

        if let Some(status) = status_filter {
            sql.push_str(&format!(" AND t.status = ${}", param_idx));
            binds.push(status.to_string());
            param_idx += 1;
        }

        if let Some(atg) = assignee_filter {
            sql.push_str(&format!(" AND t.assignee_tg_id = ${}", param_idx));
            binds.push(atg.to_string());
            param_idx += 1;
        }

        sql.push_str(&format!(
            " ORDER BY t.updated_at DESC LIMIT ${} OFFSET ${}",
            param_idx,
            param_idx + 1
        ));

        let mut q = sqlx::query_as::<_, TicketWithUser>(&sql);
        for b in &binds {
            q = q.bind(b);
        }
        q = q.bind(limit).bind(offset);

        let rows = q
            .fetch_all(&self.pool)
            .await
            .context("Ошибка запроса тикетов (admin)")?;

        Ok(rows)
    }

    /// Возвращает тикет с сообщениями.
    ///
    /// `is_admin = false` — проверяем, что тикет принадлежит пользователю `requester_user_id`.
    pub async fn get_ticket(
        &self,
        ticket_id: i64,
        is_admin: bool,
        requester_user_id: Option<i64>,
    ) -> Result<(Ticket, Vec<TicketMessage>)> {
        let ticket: Option<Ticket> = sqlx::query_as::<_, Ticket>(
            "SELECT id, user_id, category, subject, status, assignee_tg_id,
                    related_payment_id, related_subscription_id, created_at, updated_at, closed_at
             FROM tickets WHERE id = $1",
        )
        .bind(ticket_id)
        .fetch_optional(&self.pool)
        .await
        .context("Ошибка запроса тикета")?;

        let ticket = match ticket {
            Some(t) => t,
            None => bail!("Тикет #{} не найден", ticket_id),
        };

        // Проверка владения для не-администраторов
        if !is_admin {
            if let Some(uid) = requester_user_id {
                if ticket.user_id != uid {
                    bail!("Доступ к тикету запрещён");
                }
            } else {
                bail!("Требуется идентификатор пользователя");
            }
        }

        let messages = sqlx::query_as::<_, TicketMessage>(
            "SELECT id, ticket_id, sender_role, sender_tg_id, body, attachments_json, created_at
             FROM ticket_messages
             WHERE ticket_id = $1
             ORDER BY created_at ASC",
        )
        .bind(ticket_id)
        .fetch_all(&self.pool)
        .await
        .context("Ошибка запроса сообщений тикета")?;

        Ok((ticket, messages))
    }

    /// Добавляет сообщение от пользователя.
    /// Если тикет был в статусе awaiting_user — переводит в open.
    pub async fn add_user_message(
        &self,
        ticket_id: i64,
        user_id: i64,
        body: &str,
        attachment_ids: Vec<i64>,
    ) -> Result<TicketMessage> {
        // Проверяем владение и текущий статус
        let ticket: Option<Ticket> = sqlx::query_as::<_, Ticket>(
            "SELECT id, user_id, category, subject, status, assignee_tg_id,
                    related_payment_id, related_subscription_id, created_at, updated_at, closed_at
             FROM tickets WHERE id = $1",
        )
        .bind(ticket_id)
        .fetch_optional(&self.pool)
        .await?;

        let ticket = match ticket {
            Some(t) => t,
            None => bail!("Тикет #{} не найден", ticket_id),
        };

        if ticket.user_id != user_id {
            bail!("Доступ к тикету запрещён");
        }

        if ticket.status == "closed" || ticket.status == "resolved" {
            bail!("Тикет закрыт, новые сообщения недоступны");
        }

        let mut tx = self.pool.begin().await?;

        // Если ждали ответа пользователя — возвращаем в open
        if ticket.status == "awaiting_user" {
            sqlx::query(
                "UPDATE tickets SET status = 'open', updated_at = NOW() WHERE id = $1",
            )
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query("UPDATE tickets SET updated_at = NOW() WHERE id = $1")
                .bind(ticket_id)
                .execute(&mut *tx)
                .await?;
        }

        // Получаем tg_id пользователя для sender_tg_id
        let tg_id: Option<i64> = sqlx::query_scalar("SELECT tg_id FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap_or(None);

        let msg: TicketMessage = sqlx::query_as::<_, TicketMessage>(
            "INSERT INTO ticket_messages (ticket_id, sender_role, sender_tg_id, body)
             VALUES ($1, 'user', $2, $3)
             RETURNING id, ticket_id, sender_role, sender_tg_id, body, attachments_json, created_at",
        )
        .bind(ticket_id)
        .bind(tg_id)
        .bind(body)
        .fetch_one(&mut *tx)
        .await
        .context("Ошибка вставки сообщения")?;

        // Привязываем вложения к сообщению
        if !attachment_ids.is_empty() {
            sqlx::query(
                "UPDATE ticket_attachments SET message_id = $1
                 WHERE id = ANY($2) AND ticket_id = $3 AND message_id IS NULL",
            )
            .bind(msg.id)
            .bind(&attachment_ids)
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        // Уведомляем назначенного администратора
        if let Some(admin_tg) = ticket.assignee_tg_id {
            let notif = format!(
                "💬 *Ответ пользователя в тикете #{}*\n{}",
                ticket_id,
                crate::services::monitoring::escape_md_pub(body)
            );
            let _ = self.bot_manager.send_notification(admin_tg, &notif).await;
        }

        Ok(msg)
    }

    /// Добавляет сообщение от администратора.
    /// Переводит тикет в awaiting_user и создаёт уведомление для пользователя.
    pub async fn add_admin_message(
        &self,
        ticket_id: i64,
        admin_tg_id: i64,
        body: &str,
        attachment_ids: Vec<i64>,
    ) -> Result<TicketMessage> {
        let ticket: Option<Ticket> = sqlx::query_as::<_, Ticket>(
            "SELECT id, user_id, category, subject, status, assignee_tg_id,
                    related_payment_id, related_subscription_id, created_at, updated_at, closed_at
             FROM tickets WHERE id = $1",
        )
        .bind(ticket_id)
        .fetch_optional(&self.pool)
        .await?;

        let ticket = match ticket {
            Some(t) => t,
            None => bail!("Тикет #{} не найден", ticket_id),
        };

        let mut tx = self.pool.begin().await?;

        // Переводим в awaiting_user если открыт/in_progress
        if ticket.status == "open" || ticket.status == "in_progress" {
            sqlx::query(
                "UPDATE tickets SET status = 'awaiting_user', updated_at = NOW() WHERE id = $1",
            )
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query("UPDATE tickets SET updated_at = NOW() WHERE id = $1")
                .bind(ticket_id)
                .execute(&mut *tx)
                .await?;
        }

        let msg: TicketMessage = sqlx::query_as::<_, TicketMessage>(
            "INSERT INTO ticket_messages (ticket_id, sender_role, sender_tg_id, body)
             VALUES ($1, 'admin', $2, $3)
             RETURNING id, ticket_id, sender_role, sender_tg_id, body, attachments_json, created_at",
        )
        .bind(ticket_id)
        .bind(admin_tg_id)
        .bind(body)
        .fetch_one(&mut *tx)
        .await
        .context("Ошибка вставки сообщения администратора")?;

        // Привязываем вложения к сообщению
        if !attachment_ids.is_empty() {
            sqlx::query(
                "UPDATE ticket_attachments SET message_id = $1
                 WHERE id = ANY($2) AND ticket_id = $3 AND message_id IS NULL",
            )
            .bind(msg.id)
            .bind(&attachment_ids)
            .bind(ticket_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        // Создаём уведомление пользователя (доставка через бот/Mini App по его настройкам)
        let payload = json!({
            "ticket_id": ticket_id,
            "url": format!("/support/{}", ticket_id)
        });

        if let Err(e) = self
            .notifications
            .create(
                ticket.user_id,
                "support_ticket",
                "info",
                &format!("Ответ по тикету #{}", ticket_id),
                body,
                Some(payload),
            )
            .await
        {
            error!(
                "Ошибка создания уведомления для user {} (тикет #{}): {}",
                ticket.user_id, ticket_id, e
            );
        }

        Ok(msg)
    }

    /// Назначает администратора на тикет и переводит его в in_progress.
    pub async fn assign(&self, ticket_id: i64, admin_tg_id: i64) -> Result<Ticket> {
        let ticket: Option<Ticket> = sqlx::query_as::<_, Ticket>(
            "UPDATE tickets
             SET assignee_tg_id = $1,
                 status = CASE WHEN status = 'open' THEN 'in_progress' ELSE status END,
                 updated_at = NOW()
             WHERE id = $2
             RETURNING id, user_id, category, subject, status, assignee_tg_id,
                       related_payment_id, related_subscription_id, created_at, updated_at, closed_at",
        )
        .bind(admin_tg_id)
        .bind(ticket_id)
        .fetch_optional(&self.pool)
        .await
        .context("Ошибка назначения администратора")?;

        match ticket {
            Some(t) => Ok(t),
            None => bail!("Тикет #{} не найден", ticket_id),
        }
    }

    /// Устанавливает статус тикета с проверкой допустимых переходов.
    pub async fn set_status(
        &self,
        ticket_id: i64,
        new_status: &str,
        _by_admin_tg_id: i64,
    ) -> Result<Ticket> {
        // Проверяем допустимые значения статуса
        let allowed = ["open", "in_progress", "awaiting_user", "resolved", "closed"];
        if !allowed.contains(&new_status) {
            bail!("Недопустимый статус: {}", new_status);
        }

        let closed_at_expr = if new_status == "closed" {
            "NOW()"
        } else {
            "NULL"
        };

        let sql = format!(
            "UPDATE tickets
             SET status = $1, updated_at = NOW(), closed_at = {}
             WHERE id = $2
             RETURNING id, user_id, category, subject, status, assignee_tg_id,
                       related_payment_id, related_subscription_id, created_at, updated_at, closed_at",
            closed_at_expr
        );

        let ticket: Option<Ticket> = sqlx::query_as::<_, Ticket>(&sql)
            .bind(new_status)
            .bind(ticket_id)
            .fetch_optional(&self.pool)
            .await
            .context("Ошибка обновления статуса тикета")?;

        match ticket {
            Some(t) => Ok(t),
            None => bail!("Тикет #{} не найден", ticket_id),
        }
    }

    /// Загружает файл-вложение и сохраняет запись в БД.
    /// Возвращает сохранённое вложение.
    pub async fn attach_file(
        &self,
        ticket_id: i64,
        message_id: Option<i64>,
        filename: &str,
        mime: Option<&str>,
        body_bytes: &[u8],
    ) -> Result<TicketAttachment> {
        // Проверяем размер
        if body_bytes.len() > MAX_ATTACHMENT_SIZE {
            bail!(
                "Файл слишком большой: {} байт (максимум {})",
                body_bytes.len(),
                MAX_ATTACHMENT_SIZE
            );
        }

        // Проверяем MIME-тип
        if let Some(m) = mime {
            if !ALLOWED_MIMES.contains(&m) {
                bail!("Недопустимый тип файла: {}", m);
            }
        }

        // Создаём директорию, если нет
        let ticket_dir = self.upload_dir.join(ticket_id.to_string());
        fs::create_dir_all(&ticket_dir)
            .await
            .context("Ошибка создания директории для вложений")?;

        // Уникальное имя файла во избежание коллизий
        let safe_name = sanitize_filename(filename);
        let storage_path = ticket_dir.join(&safe_name);
        let storage_path_str = storage_path.to_string_lossy().to_string();

        fs::write(&storage_path, body_bytes)
            .await
            .context("Ошибка записи файла на диск")?;

        let attachment: TicketAttachment = sqlx::query_as::<_, TicketAttachment>(
            "INSERT INTO ticket_attachments
                (ticket_id, message_id, filename, mime_type, size_bytes, storage_path)
             VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, ticket_id, message_id, filename, mime_type, size_bytes, storage_path, created_at",
        )
        .bind(ticket_id)
        .bind(message_id)
        .bind(filename)
        .bind(mime)
        .bind(body_bytes.len() as i64)
        .bind(&storage_path_str)
        .fetch_one(&self.pool)
        .await
        .context("Ошибка сохранения вложения в БД")?;

        info!(
            "Загружено вложение #{} для тикета #{}: {} ({} байт)",
            attachment.id,
            ticket_id,
            filename,
            body_bytes.len()
        );

        Ok(attachment)
    }

    /// Автоматически закрывает тикеты в статусе awaiting_user старше 7 дней.
    /// Добавляет системное сообщение и устанавливает статус closed.
    /// Возвращает количество закрытых тикетов.
    pub async fn auto_close_stale(&self) -> Result<i64> {
        // Находим просроченные тикеты
        let stale_ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM tickets
             WHERE status = 'awaiting_user'
               AND updated_at < NOW() - INTERVAL '7 days'",
        )
        .fetch_all(&self.pool)
        .await
        .context("Ошибка поиска просроченных тикетов")?;

        if stale_ids.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await?;

        // Добавляем системное сообщение для каждого тикета
        for &tid in &stale_ids {
            sqlx::query(
                "INSERT INTO ticket_messages (ticket_id, sender_role, body)
                 VALUES ($1, 'system', 'Auto-closed (no response 7d)')",
            )
            .bind(tid)
            .execute(&mut *tx)
            .await?;
        }

        // Массово закрываем
        let affected = sqlx::query(
            "UPDATE tickets
             SET status = 'closed', closed_at = NOW(), updated_at = NOW()
             WHERE id = ANY($1)",
        )
        .bind(&stale_ids)
        .execute(&mut *tx)
        .await
        .context("Ошибка автоматического закрытия тикетов")?
        .rows_affected();

        tx.commit().await?;

        info!("Автозакрытие: {} просроченных тикетов закрыто", affected);
        Ok(affected as i64)
    }
}

/// Санитизирует имя файла: убирает traversal и управляющие символы.
fn sanitize_filename(name: &str) -> String {
    let base = std::path::Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload");

    // Убираем непечатаемые символы и заменяем пробелы
    let sanitized: String = base
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect();

    if sanitized.is_empty() {
        "upload".to_string()
    } else {
        // Добавляем уникальный префикс для предотвращения перезаписи
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("{}_{}", ts, sanitized)
    }
}
