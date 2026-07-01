# caramba-license

License control plane for Caramba Connect. It serves `POST /v1/activate`, signs
activation responses with an ed25519 key, and issues license keys for a tier and
duration. Free instances need no key and never call this server.

This crate is the signer. The panel (`apps/caramba-panel`) is the verifier: it
holds only the public key and confirms a response was issued by the real server.

## Trust model

The signed activation message binds `instance_id + license_key + tier +
expires_at + limits`. The panel passes its own `instance_id` into verification,
so a response signed for one instance cannot be replayed onto another, and the
tier, limits, or expiry cannot be edited without breaking the signature. The
canonical byte layout is frozen in `libs/caramba-shared/src/license.rs`.

### What this does and does not stop

The signature stops a third party who does not hold the private key. It blocks
forging a response, replaying a response onto a different `instance_id`, and
editing the tier, limits, or expiry of a real response.

It does not stop a determined self-hoster. In a self-hosted, offline-capable
design the verifier and its environment are both under the operator's control.
An operator can run `caramba-license keygen` with their own keypair, put their
own public key in `CARAMBA_LICENSE_PUBKEY`, point `CARAMBA_LICENSE_SERVER_URL` at
their own server, and issue themselves a Pro key. The panel will verify that
correctly because the root of trust lives in the same `.env` the operator owns.
There is no cryptographic defense against this without remote attestation.

Treat license enforcement as a deterrent against casual misuse, not as a hard
control against a motivated operator. For stronger binding, tie a paid upstream
service (relay, updates, hosted activation heartbeat) to the real platform key,
so the value the operator wants cannot be reproduced from the panel alone.

Until the real platform public key is baked into the installer
(`DEFAULT_LICENSE_PUBKEY` in `apps/caramba-installer/src/setup.rs`), every fresh
install defaults to an empty pubkey, which is unverifiable and therefore fails
safe to the Free tier. Bake the real public key before shipping.

## Key material is the operator's job

This repo does not ship real keys. Generate the signing key once, keep the
private key off the repo, and bake the public key into the installer.

### 1. Generate the signing key (run once)

```
caramba-license keygen --out /etc/caramba/license_signing_key.pem
```

This writes the private key as PKCS#8 PEM (owner read/write only on unix) and
prints `CARAMBA_LICENSE_PUBKEY` as base64. Store the private key safely. If you
lose it you must re-issue every key. If it leaks, anyone can forge Pro licenses,
so rotate it and reissue.

Print the public key again at any time:

```
caramba-license pubkey --signing-key /etc/caramba/license_signing_key.pem
caramba-license pubkey --signing-key /etc/caramba/license_signing_key.pem --pem
```

The base64 value is what the installer writes into each instance `.env` as
`CARAMBA_LICENSE_PUBKEY`.

### 2. Issue a key

```
caramba-license issue \
  --store /etc/caramba/keystore.json \
  --tier pro \
  --days 365 \
  --seats 1 \
  --note "acme corp"
```

Omit `--key` to get a random `CRMB-XXXX-XXXX-XXXX-XXXX` key, or pass `--key` to
set one. `--seats 1` is single seat: the first activation binds the instance id,
and any later activation from a different instance id is refused. `--seats 0`
allows unlimited instances. Hand the printed key to the operator of that
instance to set as `CARAMBA_LICENSE_KEY`.

List issued keys:

```
caramba-license list --store /etc/caramba/keystore.json
```

### 3. Run the server

```
caramba-license serve \
  --signing-key /etc/caramba/license_signing_key.pem \
  --store /etc/caramba/keystore.json \
  --bind 0.0.0.0:8088
```

All flags also read from env: `CARAMBA_LICENSE_SIGNING_KEY`,
`CARAMBA_LICENSE_STORE`, `CARAMBA_LICENSE_BIND`. `GET /healthz` returns `ok`.

## Activation contract

`POST /v1/activate`

Request:

```json
{ "license_key": "CRMB-....", "instance_id": "uuid-v4", "version": "0.9.48" }
```

Response (200):

```json
{
  "tier": "pro",
  "expires_at": "2027-01-01T00:00:00Z",
  "limits": {
    "max_nodes": 1000,
    "max_users": 0,
    "end_user_billing": true,
    "branding": true,
    "upstream_ads": false,
    "manual_approval": false
  },
  "signature": "base64-ed25519-signature"
}
```

`max_*` value `0` means unlimited. Error cases return a plain JSON
`{ "error": "..." }`:

- `400` license key or instance id missing
- `404` license key not recognized
- `403` license key expired
- `409` license key already activated on another instance (single seat)

The license server being down is not fatal to instances: the panel keeps serving
its last verified tier for a grace window before soft degrade. See the panel
license module for the grace and enforcement rules.
