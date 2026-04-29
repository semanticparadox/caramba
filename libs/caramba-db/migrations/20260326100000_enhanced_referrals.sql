-- Расширение реферальной системы: индивидуальные ставки и бонусы за регистрацию
CREATE TABLE IF NOT EXISTS user_referral_rates (
    user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    bonus_percent INTEGER,
    referrer_signup_bonus_cents INTEGER,
    referred_signup_bonus_cents INTEGER,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);
