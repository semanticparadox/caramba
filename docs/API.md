# Caramba API Documentation

This document explicitly lists available API endpoints, their authentication methods, and payload structures.

## Base URL
`/api`

## Authentication

| Method | Header | Used By |
| :--- | :--- | :--- |
| **Bearer Token (Node)** | `Authorization: Bearer <token>` | Agents (v2), Clients (v2) |
| **Bearer Token (Bot)** | `X-Bot-Token: <token>` | Bot Worker (v2) |
| **Internal Token** | `Authorization: Bearer <token>` | Internal Workers (Sub, Bot) |
| **API Key** | `Authorization: Bearer <key>` | Node Enrollment |

---

## 1. Node V2 API (`/api/v2/node/`)

Used by `caramba-node` running on VPN servers.

### Enrollment
**POST** `/register`
Registers a new node using an Enrollment Key.
*   **Auth:** Enrollment Key (API Key)
*   **Body:**
    ```json
    {
      "enrollment_key": "EXA-ENROLL-...",
      "hostname": "nyc-node-01",
      "ip": "1.2.3.4" (optional)
    }
    ```
*   **Response:**
    ```json
    {
      "node_id": 123,
      "join_token": "uuid-..."
    }
    ```

### Node Operations
**POST** `/heartbeat`
Sends periodic status updates.
*   **Auth:** Node Token
*   **Body:** `HeartbeatRequest` (JSON)
*   **Response:** `HeartbeatResponse` (JSON)

**GET** `/config`
Fetches the latest Sing-box configuration JSON.
*   **Auth:** Node Token
*   **Response:**
    ```json
    {
      "hash": "md5...",
      "content": { ...singbox config... }
    }
    ```

**POST** `/rotate-sni`
Triggers rotation of the Reality SNI domain.
*   **Auth:** Node Token
*   **Body:** `{"reason": "blocked"}`
*   **Response:** `{"status": "rotated", "new_sni": "..."}`

**GET** `/updates/poll`
Long-polling endpoint for config updates/commands.
*   **Auth:** Node Token
*   **Response:** `{"update": true, "message": "restart"}`

**GET** `/update-info`
Checks for agent binary updates.
*   **Auth:** Node Token
*   **Response:** `{"version": "1.6.0", "url": "...", "hash": "..."}`

**GET** `/settings`
Fetches global node settings (kill switch, decoy).
*   **Auth:** Node Token

**POST** `/logs`
Uploads recent log entries.
*   **Auth:** Node Token

---

## 2. Bot V2 API (`/api/v2/bot/`)

Endpoints used by the external `caramba-bot` worker. Authenticated via `X-Bot-Token`.

*   **GET** `/me`: Check bot identity.
*   **POST** `/user/upsert`: Create/update a Telegram user.
*   **GET** `/user/{tg_id}`: Get user details.
*   **GET** `/user/{tg_id}/subs`: Get user subscriptions.
*   **POST** `/user/gift`: Redeem a gift code.
*   **POST** `/user/{tg_id}/trial`: Activate a trial.
*   **GET** `/plans`: List available plans.
*   **GET** `/stats`: Get system stats for admin.

---

## 3. Client V2 API (`/api/v2/client/`)

Used by mobile/desktop clients for intelligent routing (optional).

*   **GET** `/recommended`: Returns optimal entry nodes based on user location (GeoIP) and server load.

---

## 4. Internal API (`/api/internal/`)

Used by trusted workers (Sub, Bot) to fetch configuration or report status.

*   **GET** `/nodes`: List active nodes and inbounds (for Sub worker).
*   **GET** `/subscription/{uuid}`: Get subscription details (for Sub worker).
*   **GET** `/users/{id}/keys`: Get user keys (for Sub worker).
*   **GET** `/workers/{role}/updates/poll`: Poll for worker self-updates.
*   **POST** `/workers/{role}/updates/report`: Report worker update status.

---

## 5. Admin API (`/api/admin/`)

Used by the Web Panel (HTMX). Session-cookie authenticated.

*   **GET/POST** `/frontends`: Manage Edge Nodes.
*   **GET** `/analytics`: Traffic stats.
