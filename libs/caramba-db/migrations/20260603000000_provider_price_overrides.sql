-- Per-payment-method price/currency overrides.
-- When a row exists for (target, provider) the checkout charges this amount+currency
-- instead of the base price. Absence of a row falls back to the base price in USD.
-- Amounts are integer minor units (cents/kopecks) to match plan_durations.price / products.price.

CREATE TABLE IF NOT EXISTS plan_duration_provider_prices (
    duration_id BIGINT NOT NULL REFERENCES plan_durations(id) ON DELETE CASCADE,
    provider    TEXT   NOT NULL,
    amount      BIGINT NOT NULL,
    currency    TEXT   NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (duration_id, provider)
);

CREATE TABLE IF NOT EXISTS product_provider_prices (
    product_id BIGINT NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    provider   TEXT   NOT NULL,
    amount     BIGINT NOT NULL,
    currency   TEXT   NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (product_id, provider)
);
