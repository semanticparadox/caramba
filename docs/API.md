# Caramba API Documentation

This document explicitly lists all available API endpoints, their authentication methods, and payload structures.

## Base URL
`/api`

## Authentication

| Method | Header | Used By |
| :--- | :--- | :--- |
| **Bearer Token** | `Authorization: Bearer <token>` | Agents (v2), Clients (v2), Bot |
| **API Key** | `Authorization: Bearer <key>` | Node Enrollment |

---

## 1. Node V2 API (`/api/v2/node/`)

Used by `caramba-agent` running on VPN nodes.

### Enrollment
**POST** `/register`
Registers a new node using an Enrollment Key.
*   **Auth:** Enrollment Key (API Key)
*   **Body:**
    ```json
    {
      "enrollment_key": "dk_...",
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
*   **Body:**
    ```json
    {
      "version": "1.5.0",
      "latency": 45.5,
      "upload": 102400,
      "download": 204800,
      "cpu_usage": 12.5,
      "memory_usage": 30.0,
      "active_connections": 5
    }
    ```

**GET** `/config`
Fetches the latest Xray/Sing-box configuration JSON.
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
*   **Response:**
    ```json
    {
      "status": "rotated",
      "new_sni": "www.example.com"
    }
    ```

**POST** `/logs`
Uploads recent log entries for debugging.
*   **Auth:** Node Token
*   **Body:** `{"logs": ["line 1", "line 2"]}`

**GET** `/update-info`
checks for agent binary updates.
*   **Auth:** Node Token
*   **Response:** `{"version": "1.6.0", "url": "...", "hash": "..."}`

---

## 2. Client V2 API (`/api/v2/client/`)

Used by mobile/desktop clients for intelligent routing.

### AI Routing
**GET** `/recommended`
Returns optimal entry nodes based on user location (GeoIP) and server load.
*   **Query Params:** `?lat=...&lon=...` (Optional)
*   **Response:**
    ```json
    {
      "user_location": { "lat": 50.0, "lon": 10.0 },
      "nodes": [
        {
          "id": 5,
          "name": "Frankfurt #1",
          "score": 12.5,
          "distance_km": 50,
          "load_pct": 20.0,
          "latency_ms": 15.0
        }
        ...
      ]
    }
    ```

---

## 3. Bot V2 API (`/api/v2/bot/`)

Internal endpoints for the Telegram Bot.

**POST** `/verify`
Checks if a Telegram User ID exists.
*   **Body:** `{"telegram_id": 123456789}`
*   **Response:**
    ```json
    {
      "verified": true,
      "user_id": 42,
      "username": "alice"
    }
    ```

---

## 4. Client API (`/api/client/`)

Authenticates via Telegram Web App (`initData`).

### Authentication
**POST** `/auth/telegram`
Exchanges Telegram `initData` for a Session JWT.
*   **Body:** `{"init_data": "query_id=..."}`
*   **Response:**
    ```json
    {
      "token": "jwt...",
      "user": { "id": 1, "balance": 5.00 }
    }
    ```

### User Data
*   **GET** `/user/profile`: Returns balance, stats, and referral code.
*   **GET** `/user/subscriptions`: Lists active VPN subscriptions.
    *   **Response:**
        ```json
        [
          {
            "id": 10,
            "status": "active",
            "subscription_url": "https://.../sub/uuid",
            "days_left": 25,
            ...
          }
        ]
        ```

### Store & Billing
*   **GET** `/plans`: Lists available subscription plans.
*   **POST** `/plans/purchase`: Buy a plan using balance.
    *   **Body:** `{"duration_id": 5}`
*   **GET** `/store/categories`: List e-commerce categories.
*   **POST** `/store/cart/add`: Add physical/digital item to cart.
*   **POST** `/store/checkout`: Pay for cart items.

---

## 5. Admin API (`/api/admin/`)

Used by the Web Panel (HTMX). Session-cookie authenticated.

*   **GET/POST** `/frontends`: Manage Edge Nodes.
*   **GET** `/analytics`: Traffic stats.
