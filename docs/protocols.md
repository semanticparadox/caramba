# Protocol Selection Guide

This guide helps you choose the right protocol for your network environment to ensure maximum stability, speed, and stealth.

## Quick Recommendation

| Network Environment | Recommended Protocol | Why? |
| :--- | :--- | :--- |
| **Home ISP (Fiber/Cable)** | **VLESS + Reality (TCP)** | Best stealth against Active Probing. Looks like normal traffic to Microsoft/Google/Apple. |
| **Mobile Data (4G/5G)** | **Hysteria2** or **TUIC v5** | UDP-based protocols are optimized for unstable, high-latency networks. Better recovery from packet loss. |
| **Restricted/Throttled** | **VLESS + gRPC + TLS** | gRPC often bypasses standard HTTPS throttling or shaping. Good for office/university networks. |
| **Advanced Masking** | **ShadowTLS** | Wraps your traffic in a real TLS handshake with a legitimate domain (e.g. cloudflare.com). Very hard to detect. |

---

## Detailed Protocol Breakdown

### 1. VLESS + Reality (The Gold Standard)
**Best for:** General use, Home Internet, High Censorship Areas.

*   **Mechanism:** Uses XTLS-Vision flow control and "Reality" security. It steals the TLS handshake of a real website (e.g., `www.microsoft.com`) and proxies traffic inside it. To a censor, it looks exactly like you are visiting that website.
*   **Pros:** Extremely stealthy. No certificate management required (self-signed is fine because it mimics the target).
*   **Cons:** Requires TCP, which can be slower on bad mobile connections compared to UDP protocols.

### 2. Hysteria2 (UDP Optimized)
**Best for:** Mobile Networks, Lossy Connections, Gaming.

*   **Mechanism:** A custom UDP protocol based on QUIC, designed to brute-force through lossy networks with aggressive congestion control.
*   **Pros:** Very fast on unstable lines. Resumes connections instantly (0-RTT).
*   **Cons:** High UDP usage can trigger "UDP Throttle" or blocking on some strict corporate firewalls.

### 3. TUIC v5
**Best for:** Mobile Networks (Alternative to Hysteria).

*   **Mechanism:** Another UDP-based protocol using QUIC. Similar to Hysteria2 but with a different congestion control algorithm (BBR).
*   **Pros:** Low latency, high throughput.
*   **Cons:** Less mature ecosystem than VLESS/Hysteria.

### 4. VLESS + gRPC + TLS
**Best for:** CDN workers, Cloudflare routing, or Throttled HTTPS.

*   **Mechanism:** Encapsulates traffic in gRPC calls over HTTP/2.
*   **Pros:** Can be routed through CDNs (Cloudflare, GCore) to hide the server IP.
*   **Cons:** Higher overhead (CPU usage) than TCP.

### 5. ShadowTLS
**Best for:** Advanced users facing sophisticated DPI (Deep Packet Inspection).

*   **Mechanism:** A proxy wrapper. It performs a *real* handshake with a legitimate domain (e.g., `pay.google.com`), then hijacks the connection after the handshake is complete.
*   **Pros:** The handshake is indistinguishable from a real one because it *is* a real one.
*   **Cons:** More complex to set up.

---

## Routing Strategies

### Global Mode
All traffic from your device goes through the VPN.
*   **Use when:** You want complete privacy or are on an untrusted public Wi-Fi.

### Smart Mode (Split-Tunneling)
Only blocked domains go through the VPN. Domestic traffic (e.g., banking, local news, gov sites) goes directly to the internet.
*   **Use when:** You want to access local services without triggering fraud alerts or slowing down local downloads.
*   **Mechanism:** The client uses `geosite` and `geoip` rules to decide where to route packets.
