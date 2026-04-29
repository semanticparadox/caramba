/// Get translated string. `lang` is user's language_code (e.g. Some("ru"), Some("en"), None).
/// Returns Russian for "ru*", English for everything else.
pub fn t(lang: Option<&str>, key: &str) -> &'static str {
    let is_ru = lang.map_or(true, |l| l.starts_with("ru")); // Default to Russian
    match (key, is_ru) {
        // =====================================================================
        // Keyboard buttons (main menu)
        // =====================================================================
        ("kb.buy_sub", true) => "🛍 Купить подписку",
        ("kb.buy_sub", false) => "🛍 Buy Subscription",

        ("kb.my_services", true) => "🔐 Мои сервисы",
        ("kb.my_services", false) => "🔐 My Services",

        ("kb.digital_store", true) => "📦 Магазин",
        ("kb.digital_store", false) => "📦 Digital Store",

        ("kb.my_profile", true) => "👤 Мой профиль",
        ("kb.my_profile", false) => "👤 My Profile",

        ("kb.bonuses", true) => "🎁 Бонусы / Рефералы",
        ("kb.bonuses", false) => "🎁 Bonuses / Referral",

        ("kb.support", true) => "❓ Поддержка",
        ("kb.support", false) => "❓ Support",

        // =====================================================================
        // Keyboard buttons (terms)
        // =====================================================================
        ("kb.accept", true) => "✅ Принять",
        ("kb.accept", false) => "✅ Accept",

        ("kb.decline", true) => "❌ Отклонить",
        ("kb.decline", false) => "❌ Decline",

        // =====================================================================
        // Keyboard buttons (cart)
        // =====================================================================
        ("kb.view_cart", true) => "🛒 Корзина",
        ("kb.view_cart", false) => "🛒 View Cart",

        ("kb.checkout", true) => "✅ Оформить",
        ("kb.checkout", false) => "✅ Checkout",

        ("kb.clear_cart", true) => "🗑️ Очистить",
        ("kb.clear_cart", false) => "🗑️ Clear Cart",

        // =====================================================================
        // Keyboard buttons (profile / subscription actions)
        // =====================================================================
        ("kb.topup", true) => "💳 Пополнить баланс",
        ("kb.topup", false) => "💳 Top-up Balance",

        ("kb.edit_note", true) => "📝 Заметка",
        ("kb.edit_note", false) => "📝 Edit Note",

        ("kb.devices", true) => "📱 Устройства",
        ("kb.devices", false) => "📱 Connected Devices",

        ("kb.get_config", true) => "🔗 Конфиг",
        ("kb.get_config", false) => "🔗 Get Config",

        ("kb.get_links", true) => "🔗 Ссылки",
        ("kb.get_links", false) => "🔗 Get Links",

        ("kb.json_profile", true) => "📄 JSON Профиль",
        ("kb.json_profile", false) => "📄 JSON Profile",

        ("kb.extend", true) => "⏳ Продлить",
        ("kb.extend", false) => "⏳ Extend",

        ("kb.activate", true) => "▶️ Активировать",
        ("kb.activate", false) => "▶️ Activate",

        ("kb.make_gift", true) => "🎁 Подарочный код",
        ("kb.make_gift", false) => "🎁 Make Gift Code",

        ("kb.my_gifts", true) => "🎁 Мои подарки",
        ("kb.my_gifts", false) => "🎁 My Gift Codes",

        ("kb.enter_promo", true) => "🎟 Промокод",
        ("kb.enter_promo", false) => "🎟 Enter Promo Code",

        ("kb.edit_ref_code", true) => "🔗 Изменить код",
        ("kb.edit_ref_code", false) => "🔗 Edit My Code (Alias)",

        ("kb.enter_referrer", true) => "🎁 Код реферера",
        ("kb.enter_referrer", false) => "🎁 Enter Referrer Code",

        ("kb.contact_support", true) => "💬 Связаться с поддержкой",
        ("kb.contact_support", false) => "💬 Contact Support",

        ("kb.reset_sessions", true) => "☠️ Сбросить сессии",
        ("kb.reset_sessions", false) => "☠️ Reset Sessions",

        ("kb.back_services", true) => "« Назад к сервисам",
        ("kb.back_services", false) => "« Back to Services",

        ("kb.back_categories", true) => "« Назад к категориям",
        ("kb.back_categories", false) => "« Back to Categories",

        ("kb.back", true) => "« Назад",
        ("kb.back", false) => "« Back",

        ("kb.buy_now", true) => "💰 Купить",
        ("kb.buy_now", false) => "💰 Buy Now",

        ("kb.prev", true) => "⬅️ Назад",
        ("kb.prev", false) => "⬅️ Prev",

        ("kb.next", true) => "Далее ➡️",
        ("kb.next", false) => "Next ➡️",

        ("kb.devices_short", true) => "📱 Устройства",
        ("kb.devices_short", false) => "📱 Devices",

        // =====================================================================
        // Payment buttons (brand names stay, labels translated)
        // =====================================================================
        ("kb.pay_cryptobot", true) => "🪙 Crypto (USDT/TON)",
        ("kb.pay_cryptobot", false) => "🪙 Crypto (USDT/TON)",

        ("kb.pay_nowpayments", true) => "⚡ Crypto (Altcoins)",
        ("kb.pay_nowpayments", false) => "⚡ Crypto (Altcoins)",

        ("kb.pay_crystal", true) => "🇷🇺 Карты (RUB/СБП)",
        ("kb.pay_crystal", false) => "🇷🇺 Cards (RUB/SBP)",

        ("kb.pay_stripe", true) => "🌍 Карты (USD)",
        ("kb.pay_stripe", false) => "🌍 Global Cards (USD)",

        ("kb.pay_stars", true) => "⭐️ Telegram Stars",
        ("kb.pay_stars", false) => "⭐️ Telegram Stars",

        // =====================================================================
        // Welcome / start messages
        // =====================================================================
        ("msg.hello", true) => "👋 <b>Привет, {0}!</b>\n\nИспользуйте меню для управления VPN-подписками.",
        ("msg.hello", false) => "👋 <b>Hello, {0}!</b>\n\nUse the menu below to manage your VPN subscriptions and digital goods.",

        ("msg.welcome", true) => "👋 <b>Добро пожаловать!</b>\n\nИспользуйте меню для управления VPN-подписками.",
        ("msg.welcome", false) => "👋 <b>Welcome!</b>\n\nUse the menu below to manage your VPN subscriptions and digital goods.",

        ("msg.menu_button", true) => "Открыть",
        ("msg.menu_button", false) => "Open",

        // =====================================================================
        // Access / bans
        // =====================================================================
        ("msg.access_denied", true) => "🚫 *Доступ запрещён*\n\nВаш аккаунт заблокирован\\.",
        ("msg.access_denied", false) => "🚫 *Access Denied*\n\nYour account has been banned\\.",

        ("msg.account_banned", true) => "🚫 <b>Аккаунт заблокирован</b> за спам.",
        ("msg.account_banned", false) => "🚫 <b>Account Banned</b> due to spam/botting.",

        // =====================================================================
        // Language / Terms
        // =====================================================================
        ("msg.select_language", true) => "🌐 <b>Пожалуйста, выберите язык / Please select your language:</b>",
        ("msg.select_language", false) => "🌐 <b>Please select your language / Пожалуйста, выберите язык:</b>",

        ("msg.terms_header", true) => "📜 <b>Условия использования</b>",
        ("msg.terms_header", false) => "📜 <b>Terms of Service</b>",

        ("msg.terms_accept_prompt", true) => "Примите условия для продолжения.",
        ("msg.terms_accept_prompt", false) => "Please accept the terms to continue.",

        ("msg.must_accept_terms", true) => "Необходимо принять условия.",
        ("msg.must_accept_terms", false) => "You must accept terms to proceed.",

        // Предупреждение о непринятых условиях использования (шаблон: {0}=текущее, {1}=макс)
        ("msg.tos_warning", true) => "⚠️ Пожалуйста, примите условия использования. Предупреждение {0}/{1}. После {1} предупреждений доступ будет заблокирован.",
        ("msg.tos_warning", false) => "⚠️ Please accept the Terms of Service. Warning {0}/{1}. After {1} warnings your access will be blocked.",

        // =====================================================================
        // Payments
        // =====================================================================
        ("msg.payment_success", true) => "✅ Оплата успешна! Баланс обновлён.",
        ("msg.payment_success", false) => "✅ Payment successful! Balance updated.",

        ("msg.payment_error", true) => "❌ Ошибка оплаты. Обратитесь в поддержку.",
        ("msg.payment_error", false) => "❌ Error processing payment. Please contact support.",

        ("msg.cart_cleared", true) => "🗑️ Корзина очищена.",
        ("msg.cart_cleared", false) => "🗑️ Cart cleared.",

        ("msg.sessions_killed", true) => "🔌 Все устройства отключены от этой подписки.",
        ("msg.sessions_killed", false) => "🔌 All devices disconnected from this subscription.",

        ("msg.topup_menu", true) => "💳 *Выберите способ пополнения:*",
        ("msg.topup_menu", false) => "💳 *Choose Top-up Method:*",

        ("msg.select_amount_cryptobot", true) => "🔹 *Выберите сумму для CryptoBot:*",
        ("msg.select_amount_cryptobot", false) => "🔹 *Select amount for CryptoBot:*",

        ("msg.select_amount_nowpayments", true) => "🔹 *Выберите сумму для NOWPayments:*",
        ("msg.select_amount_nowpayments", false) => "🔹 *Select amount for NOWPayments:*",

        ("msg.select_amount_crystal", true) => "🔹 *Выберите сумму для CrystalPay (Карты/СБП):*",
        ("msg.select_amount_crystal", false) => "🔹 *Select amount for CrystalPay (Cards/SBP):*",

        ("msg.select_amount_stripe", true) => "🔹 *Выберите сумму для Stripe:*",
        ("msg.select_amount_stripe", false) => "🔹 *Select amount for Stripe:*",

        ("msg.select_amount_stars", true) => "🔹 *Выберите сумму через Stars:*",
        ("msg.select_amount_stars", false) => "🔹 *Select amount via Stars:*",

        ("msg.invoice_created", true) => "💳 Счёт на *${0}* создан\\!",
        ("msg.invoice_created", false) => "💳 Invoice for *${0}* created\\!",

        ("msg.balance_topup", true) => "Пополнение баланса",
        ("msg.balance_topup", false) => "Balance Top-up",

        ("msg.topup_description", true) => "Пополнить баланс на ${0}",
        ("msg.topup_description", false) => "Top-up balance by ${0}",

        // =====================================================================
        // Plans / Buy subscription
        // =====================================================================
        ("msg.no_plans", true) => "❌ Нет доступных тарифов.",
        ("msg.no_plans", false) => "❌ No active plans available at the moment.",

        ("msg.no_plans_short", true) => "❌ Нет тарифов.",
        ("msg.no_plans_short", false) => "❌ No active plans.",

        ("msg.choose_plan_extend", true) => "💎 *Выберите тариф для продления:*\n\n",
        ("msg.choose_plan_extend", false) => "💎 *Choose Plan to Extend:*\n\n",

        ("msg.purchase_success", true) => "✅ *Покупка успешна\\! Подписка уже активна\\.*",
        ("msg.purchase_success", false) => "✅ *Purchase Successful\\! Your subscription is now active\\.*",

        ("msg.purchase_success_short", true) => "✅ Успех!",
        ("msg.purchase_success_short", false) => "✅ Success!",

        // Кнопки выбора при покупке тарифа
        ("btn.buy_for_myself", true) => "🛒 Купить для себя",
        ("btn.buy_for_myself", false) => "🛒 Buy for myself",

        ("btn.buy_as_gift", true) => "🎁 Купить в подарок",
        ("btn.buy_as_gift", false) => "🎁 Buy as gift",

        ("msg.buy_for_whom", true) => "🛒 *Для кого покупаем?*\n\nВыберите: для себя \\(подписка сразу активна\\) или как подарок \\(получите код для передачи\\)\\.",
        ("msg.buy_for_whom", false) => "🛒 *Who is this for?*\n\nChoose: for yourself \\(subscription activates immediately\\) or as a gift \\(you'll receive a code to share\\)\\.",

        // =====================================================================
        // Digital Store
        // =====================================================================
        ("msg.store_empty", true) => "❌ Магазин пуст.",
        ("msg.store_empty", false) => "❌ The store is currently empty.",

        ("msg.store_welcome", true) => "📦 *Добро пожаловать в магазин*\\nВыберите категорию:",
        ("msg.store_welcome", false) => "📦 *Welcome to the Digital Store*\\nSelect a category to browse:",

        ("msg.no_products", true) => "📦 Нет товаров в этой категории.",
        ("msg.no_products", false) => "📦 No products in this category.",

        ("msg.products", true) => "📦 *Товары:*\n\n",
        ("msg.products", false) => "📦 *Products:*\n\n",

        ("msg.product_details", true) => "📦 *Детали товара*\n\n(Детали здесь...)",
        ("msg.product_details", false) => "📦 *Product Details*\n\n(Details would go here...)",

        ("msg.purchase_product_success", true) => "✅ Покупка успешна!",
        ("msg.purchase_product_success", false) => "✅ Purchase successful!",

        ("msg.no_content", true) => "Нет дополнительного контента",
        ("msg.no_content", false) => "No additional content",

        // =====================================================================
        // Cart
        // =====================================================================
        ("msg.cart_empty", true) => "🛒 Корзина пуста.",
        ("msg.cart_empty", false) => "🛒 Your cart is empty.",

        ("msg.cart_header", true) => "🛒 *ВАША КОРЗИНА*\n\n",
        ("msg.cart_header", false) => "🛒 *YOUR SHOPPING CART*\n\n",

        // =====================================================================
        // Services
        // =====================================================================
        ("msg.my_services", true) => "🔐 *МОИ СЕРВИСЫ*\n\n",
        ("msg.my_services", false) => "🔐 *MY SERVICES*\n\n",

        ("msg.no_subscriptions", true) => "📡 VPN: ❌ *Нет подписок*\n\n",
        ("msg.no_subscriptions", false) => "📡 VPN Status: ❌ *No Subscriptions*\n\n",

        ("msg.no_active_subs", true) => "❌ У вас нет активных подписок.",
        ("msg.no_active_subs", false) => "❌ You have no active subscriptions.",

        ("msg.plan", true) => "Тариф:",
        ("msg.plan", false) => "Plan:",

        ("msg.traffic_used", true) => "Трафик:",
        ("msg.traffic_used", false) => "Traffic Used:",

        ("msg.traffic", true) => "Трафик:",
        ("msg.traffic", false) => "Traffic:",

        ("msg.expires", true) => "Истекает:",
        ("msg.expires", false) => "Expires:",

        ("msg.no_expiration", true) => "Без срока",
        ("msg.no_expiration", false) => "No expiration",

        ("msg.traffic_plan", true) => "Трафик-план",
        ("msg.traffic_plan", false) => "Traffic Plan",

        ("msg.duration", true) => "Срок:",
        ("msg.duration", false) => "Duration:",

        ("msg.days", true) => "дней",
        ("msg.days", false) => "days",

        ("msg.starts_on_activation", true) => "начинается после активации",
        ("msg.starts_on_activation", false) => "starts on activation",

        ("msg.premium_access", true) => "Премиум доступ",
        ("msg.premium_access", false) => "Premium access",

        // =====================================================================
        // Notes / Transfer
        // =====================================================================
        ("msg.note_updated", true) => "✅ Заметка обновлена!",
        ("msg.note_updated", false) => "✅ Note updated!",

        ("msg.reply_with_note", true) => "Ответьте на это сообщение с заметкой для подписки #{0}.",
        ("msg.reply_with_note", false) => "Reply to this message with your note for Subscription #{0}.",

        ("msg.transfer_success", true) => "✅ Подписка \\#{0} передана {1}\\!",
        ("msg.transfer_success", false) => "✅ Subscription \\#{0} transferred to {1} successfully\\!",

        ("msg.transfer_failed", true) => "❌ Ошибка передачи: {0}",
        ("msg.transfer_failed", false) => "❌ Transfer failed: {0}",

        ("msg.transfer_prompt", true) => "➡️ *Передача подписки*\n\nОтветьте с именем пользователя для подписки #{0}.",
        ("msg.transfer_prompt", false) => "➡️ *Transfer Subscription*\n\nReply with username for Subscription #{0}.",

        // =====================================================================
        // Gift codes / Promo
        // =====================================================================
        ("msg.enter_gift_code", true) => "🎟 Введите подарочный код:",
        ("msg.enter_gift_code", false) => "🎟 Enter your Gift Code below:",

        ("msg.redeem_gift", true) => "🎟 *Активировать подарочный код*\n\nОтветьте на это сообщение с вашим кодом (напр\\. `EXA-GIFT-XYZ`)\\.",
        ("msg.redeem_gift", false) => "🎟 *Redeem Gift Code*\n\nPlease reply to this message with your code (e.g., `EXA-GIFT-XYZ`).",

        ("msg.gift_created", true) => "🎁 *Подарочный код создан!*\n\nКод: `{0}`\n\nПоделитесь этим кодом.",
        ("msg.gift_created", false) => "🎁 *Gift Code Created!*\n\nCode: `{0}`\n\nShare this code.",

        ("msg.code_generated", true) => "✅ Код создан!",
        ("msg.code_generated", false) => "✅ Code Generated!",

        ("msg.no_gift_codes", true) => "🎁 У вас нет неактивированных подарочных кодов.",
        ("msg.no_gift_codes", false) => "🎁 You have no unredeemed gift codes.",

        ("msg.my_gift_codes_header", true) => "🎁 *Мои подарочные коды* \\(неактивированные\\):\n\n",
        ("msg.my_gift_codes_header", false) => "🎁 *My Gift Codes* \\(Unredeemed\\):\n\n",

        ("msg.gift_error", true) => "❌ Ошибка загрузки подарочных кодов.",
        ("msg.gift_error", false) => "❌ Error fetching gift codes.",

        ("msg.redemption_success", true) => "✅ *Успех\\!*\n\n{0}",
        ("msg.redemption_success", false) => "✅ *Success\\!*\n\n{0}",

        ("msg.redemption_failed", true) => "❌ Ошибка активации: {0}",
        ("msg.redemption_failed", false) => "❌ Redemption Failed: {0}",

        // =====================================================================
        // Referral / Bonus
        // =====================================================================
        ("msg.bonus_header", true) => "🎁 *БОНУСНАЯ ПРОГРАММА*\n\n🤝 *Приглашайте друзей и зарабатывайте\\!*\nВы получаете *10%* от *КАЖДОЙ* покупки ваших друзей\\.\n\n",
        ("msg.bonus_header", false) => "🎁 *BONUS PROGRAM*\n\n🤝 *Invite friends and earn money\\!*\nYou get *10%* from *EVERY* purchase your friends make\\.\n\n",

        ("msg.your_stats", true) => "📊 *Ваша статистика:*\n",
        ("msg.your_stats", false) => "📊 *Your Statistics:*\n",

        ("msg.referrals_joined", true) => "👥 Рефералов: *{0}*\n",
        ("msg.referrals_joined", false) => "👥 Referrals joined: *{0}*\n",

        ("msg.total_earned", true) => "💰 Заработано: *${0}\\.{1}*\n\n",
        ("msg.total_earned", false) => "💰 Total earned: *${0}\\.{1}*\n\n",

        ("msg.promo_data", true) => "🔗 *Ваши промо данные:*\nКод: `{0}`\nСсылка: `{1}`\n\n_Поделитесь ссылкой или кодом\\!_",
        ("msg.promo_data", false) => "🔗 *Your Promo Data:*\nCode: `{0}`\nLink: `{1}`\n\n_Share your link or code to start earning\\!_",

        ("msg.alias_invalid_length", true) => "❌ *Неверная длина*\n\nКод должен быть от 3 до 32 символов\\.",
        ("msg.alias_invalid_length", false) => "❌ *Invalid Length*\n\nReferral alias must be between 3 and 32 characters\\.",

        ("msg.alias_invalid_chars", true) => "❌ *Недопустимые символы*\n\nКод может содержать только буквы, цифры и подчёркивания\\.",
        ("msg.alias_invalid_chars", false) => "❌ *Invalid Characters*\n\nReferral alias can only contain letters, numbers, and underscores\\.",

        ("msg.alias_updated", true) => "✅ *Код обновлён\\!*\n\nВаши новые данные:\nКод: `{0}`\nСсылка: `{1}`",
        ("msg.alias_updated", false) => "✅ *Referral Alias Updated\\!*\n\nYour new data:\nCode: `{0}`\nLink: `{1}`",

        ("msg.alias_taken", true) => "❌ *Ошибка обновления*\n\nЭтот код уже занят или недействителен\\.",
        ("msg.alias_taken", false) => "❌ *Update Failed*\n\nThis alias might already be taken or invalid\\.",

        ("msg.referrer_linked", true) => "✅ *Реферер привязан\\!*\n\nВы успешно указали реферера\\.",
        ("msg.referrer_linked", false) => "✅ *Referrer Linked\\!*\n\nYou've successfully set your referrer\\.",

        ("msg.linking_failed", true) => "❌ Ошибка привязки: {0}",
        ("msg.linking_failed", false) => "❌ Linking Failed: {0}",

        ("msg.edit_ref_alias", true) => "EDIT REFERRAL ALIAS",
        ("msg.edit_ref_alias", false) => "EDIT REFERRAL ALIAS",

        ("msg.enter_referrer_code", true) => "Enter Referrer Code",
        ("msg.enter_referrer_code", false) => "Enter Referrer Code",

        // =====================================================================
        // Support
        // =====================================================================
        ("msg.support_not_configured", true) => "❌ Контакт поддержки не настроен.",
        ("msg.support_not_configured", false) => "❌ Support contact is not configured yet.",

        ("msg.support_prompt", true) => "Нужна помощь? Нажмите кнопку ниже:",
        ("msg.support_prompt", false) => "Need help? Click the button below to contact support:",

        // =====================================================================
        // Subscription activation / extend
        // =====================================================================
        ("msg.activated", true) => "✅ Активировано!",
        ("msg.activated", false) => "✅ Activated!",

        ("msg.sub_activated", true) => "🚀 *Подписка активирована!*\nИстекает: `{0}`",
        ("msg.sub_activated", false) => "🚀 *Subscription Activated!*\nExpires: `{0}`",

        ("msg.extended", true) => "✅ Продлено!",
        ("msg.extended", false) => "✅ Extended!",

        ("msg.sub_extended", true) => "✅ *Подписка продлена!*",
        ("msg.sub_extended", false) => "✅ *Subscription Extended!*",

        // =====================================================================
        // Connection links / config
        // =====================================================================
        ("msg.fetching_links", true) => "Загрузка ссылок...",
        ("msg.fetching_links", false) => "Fetching links...",

        ("msg.no_links", true) => "❌ Нет доступных ссылок.",
        ("msg.no_links", false) => "❌ No connection links available.",

        ("msg.your_links", true) => "🔗 *Ваши ссылки подключения:*\n\n",
        ("msg.your_links", false) => "🔗 *Your Connection Links:*\n\n",

        ("msg.links_error", true) => "❌ Ошибка загрузки ссылок.",
        ("msg.links_error", false) => "❌ Failed to fetch links.",

        ("msg.generating_profile", true) => "Генерация профиля...",
        ("msg.generating_profile", false) => "Generating profile...",

        ("msg.your_profile_file", true) => "📂 <b>Ваш профиль CARAMBA</b>\n\nИмпортируйте файл в Sing-box, Nekobox или Hiddify.",
        ("msg.your_profile_file", false) => "📂 <b>Your CARAMBA Profile</b>\n\nImport this file into Sing-box, Nekobox, or Hiddify.",

        ("msg.profile_error", true) => "❌ Ошибка генерации профиля.",
        ("msg.profile_error", false) => "❌ Failed to generate profile.",

        // =====================================================================
        // User profile
        // =====================================================================
        ("msg.user_profile", true) => "👤 *ПРОФИЛЬ*\n\n🆔 ID: `{0}`\n💰 Баланс: `${1}`\n\n_Используйте 'Мои сервисы' для управления подписками\\._",
        ("msg.user_profile", false) => "👤 *USER PROFILE*\n\n🆔 ID: `{0}`\n💰 Balance: `${1}`\n\n_Use 'My Services' to manage subscriptions and products\\._",

        // =====================================================================
        // Devices
        // =====================================================================
        ("msg.devices_header", true) => "📱 *ПОДКЛЮЧЁННЫЕ УСТРОЙСТВА*\\n\\n",
        ("msg.devices_header", false) => "📱 *CONNECTED DEVICES*\\n\\n",

        ("msg.device_limit", true) => "🔢 *Лимит устройств:* `{0}`\\n",
        ("msg.device_limit", false) => "🔢 *Device Limit:* `{0}`\\n",

        ("msg.active_devices", true) => "✅ *Активных:* `{0}`\\n\\n",
        ("msg.active_devices", false) => "✅ *Active Devices:* `{0}`\\n\\n",

        ("msg.no_devices", true) => "_Нет подключённых устройств\\._\\n\\n",
        ("msg.no_devices", false) => "_No devices currently connected\\._\\n\\n",

        ("msg.recent_connections", true) => "🌐 *Последние подключения:*\\n",
        ("msg.recent_connections", false) => "🌐 *Recent Connections:*\\n",

        ("msg.min_ago", true) => "мин. назад",
        ("msg.min_ago", false) => "min ago",

        ("msg.devices_for_sub", true) => "📱 *Устройства для подписки \\#{0}*\n",
        ("msg.devices_for_sub", false) => "📱 *Active Devices for Subscription \\#{0}*\n",

        ("msg.no_sessions", true) => "Нет сессий за последние 15 минут\\.",
        ("msg.no_sessions", false) => "No active sessions detected in the last 15 minutes\\.",

        ("msg.select_sub_devices", true) => "📱 *Выберите подписку для просмотра устройств:*",
        ("msg.select_sub_devices", false) => "📱 *Select a subscription to view devices:*",

        ("msg.mins_ago", true) => "мин\\. назад",
        ("msg.mins_ago", false) => "mins ago",

        // =====================================================================
        // Pay button labels
        // =====================================================================
        ("msg.pay_cryptobot", true) => "🔗 Оплатить через CryptoBot",
        ("msg.pay_cryptobot", false) => "🔗 Pay with CryptoBot",

        ("msg.pay_nowpayments", true) => "🔗 Оплатить через NOWPayments",
        ("msg.pay_nowpayments", false) => "🔗 Pay with NOWPayments",

        ("msg.pay_crystal", true) => "🔗 Оплатить картой (CrystalPay)",
        ("msg.pay_crystal", false) => "🔗 Pay with Card (CrystalPay)",

        ("msg.pay_stripe", true) => "🔗 Оплатить через Stripe",
        ("msg.pay_stripe", false) => "🔗 Pay with Stripe",

        // =====================================================================
        // Misc
        // =====================================================================
        ("msg.view_product", true) => "Подробнее: {0}",
        ("msg.view_product", false) => "View: {0}",

        // =====================================================================
        // Balance & billing notifications
        // =====================================================================

        // Предупреждение о низком балансе (шаблон: {0}=текущий баланс в долларах, {1}=имя плана)
        ("msg.balance_low_warning", true) => "⚠️ *Баланс заканчивается*\n\nВаш текущий баланс: *${0}*\n\nДля автопродления подписки «{1}» необходимо пополнить счёт\\. Пополните баланс заранее, чтобы не потерять доступ\\.",
        ("msg.balance_low_warning", false) => "⚠️ *Balance Running Low*\n\nYour current balance: *${0}*\n\nTop up to ensure auto\\-renewal of your «{1}» subscription and avoid losing access\\.",

        // Успешное автопродление (шаблон: {0}=имя плана, {1}=дата истечения, {2}=сумма)
        ("msg.auto_renewal_success", true) => "✅ *Подписка автоматически продлена*\n\n💎 Тариф: *{0}*\n📅 Действует до: *{1}*\n💳 Списано: *${2}*",
        ("msg.auto_renewal_success", false) => "✅ *Subscription Auto\\-Renewed*\n\n💎 Plan: *{0}*\n📅 Valid until: *{1}*\n💳 Charged: *${2}*",

        // Ошибка автопродления — недостаточно средств (шаблон: {0}=имя плана, {1}=баланс, {2}=нужная сумма)
        ("msg.auto_renewal_failed", true) => "⚠️ *Автопродление не выполнено*\n\n💎 Тариф: *{0}*\n💰 Баланс: *${1}*\n💳 Требуется: *${2}*\n\nПополните баланс, чтобы продолжить пользоваться VPN\\.",
        ("msg.auto_renewal_failed", false) => "⚠️ *Auto\\-Renewal Failed*\n\n💎 Plan: *{0}*\n💰 Balance: *${1}*\n💳 Required: *${2}*\n\nPlease top up your account to keep your VPN access\\.",

        // Платёж отклонён провайдером (шаблон: {0}=сумма, {1}=провайдер)
        ("msg.payment_declined", true) => "❌ *Платёж отклонён*\n\nПлатёж на сумму *${0}* через *{1}* не прошёл\\.\n\nПопробуйте другой способ оплаты или обратитесь в поддержку\\.",
        ("msg.payment_declined", false) => "❌ *Payment Declined*\n\nYour payment of *${0}* via *{1}* was declined\\.\n\nPlease try a different payment method or contact support\\.",

        // =====================================================================
        // Admin menu
        // =====================================================================
        ("admin.menu.title", true) => "<b>Панель администратора</b>\n\nВыберите раздел:",
        ("admin.menu.title", false) => "<b>Admin Panel</b>\n\nSelect a section:",

        ("admin.menu.tickets", true) => "Тикеты",
        ("admin.menu.tickets", false) => "Tickets",

        ("admin.menu.broadcast", true) => "Broadcast уведомление",
        ("admin.menu.broadcast", false) => "Broadcast Notification",

        ("admin.menu.stats", true) => "Статистика",
        ("admin.menu.stats", false) => "Statistics",

        ("admin.menu.moderation", true) => "Модерация (скоро)",
        ("admin.menu.moderation", false) => "Moderation (soon)",

        // =====================================================================
        // Admin tickets
        // =====================================================================
        ("admin.tickets.empty", true) => "Тикетов по данному фильтру не найдено.",
        ("admin.tickets.empty", false) => "No tickets found for this filter.",

        ("admin.tickets.list_title", true) => "<b>Тикеты</b>",
        ("admin.tickets.list_title", false) => "<b>Tickets</b>",

        ("admin.tickets.no_admin", true) => "Недоступно.",
        ("admin.tickets.no_admin", false) => "Access denied.",

        // =====================================================================
        // Admin ticket detail
        // =====================================================================
        ("admin.ticket.detail_subject", true) => "Тема",
        ("admin.ticket.detail_subject", false) => "Subject",

        ("admin.ticket.detail_status", true) => "Статус",
        ("admin.ticket.detail_status", false) => "Status",

        ("admin.ticket.detail_category", true) => "Категория",
        ("admin.ticket.detail_category", false) => "Category",

        ("admin.ticket.detail_user", true) => "Пользователь",
        ("admin.ticket.detail_user", false) => "User",

        ("admin.ticket.detail_assignee", true) => "Назначен",
        ("admin.ticket.detail_assignee", false) => "Assigned to",

        ("admin.ticket.detail_messages", true) => "Последние сообщения",
        ("admin.ticket.detail_messages", false) => "Recent messages",

        ("admin.ticket.detail_unassigned", true) => "Не назначен",
        ("admin.ticket.detail_unassigned", false) => "Unassigned",

        // =====================================================================
        // Admin reply FSM
        // =====================================================================
        ("admin.reply.prompt", true) => "Введите текст ответа:",
        ("admin.reply.prompt", false) => "Enter your reply:",

        ("admin.reply.sent", true) => "Ответ отправлен.",
        ("admin.reply.sent", false) => "Reply sent.",

        ("admin.reply.cancelled", true) => "Ответ отменён.",
        ("admin.reply.cancelled", false) => "Reply cancelled.",

        // =====================================================================
        // Admin broadcast FSM
        // =====================================================================
        ("admin.broadcast.step_segment", true) => "Шаг 1/5 — Кому отправить?\n\nВыберите сегмент аудитории:",
        ("admin.broadcast.step_segment", false) => "Step 1/5 — Who to notify?\n\nSelect audience segment:",

        ("admin.broadcast.step_category", true) => "Шаг 2/5 — Категория уведомления:",
        ("admin.broadcast.step_category", false) => "Step 2/5 — Notification category:",

        ("admin.broadcast.step_severity", true) => "Шаг 3/5 — Уровень важности:",
        ("admin.broadcast.step_severity", false) => "Step 3/5 — Severity level:",

        ("admin.broadcast.step_title", true) => "Шаг 4/5 — Введите заголовок уведомления:",
        ("admin.broadcast.step_title", false) => "Step 4/5 — Enter notification title:",

        ("admin.broadcast.step_body", true) => "Шаг 5/5 — Введите текст уведомления:",
        ("admin.broadcast.step_body", false) => "Step 5/5 — Enter notification body:",

        ("admin.broadcast.confirm_prompt", true) => "Подтвердите отправку broadcast:",
        ("admin.broadcast.confirm_prompt", false) => "Confirm broadcast send:",

        ("admin.broadcast.sent", true) => "Broadcast отправлен.",
        ("admin.broadcast.sent", false) => "Broadcast sent.",

        ("admin.broadcast.cancelled", true) => "Broadcast отменён.",
        ("admin.broadcast.cancelled", false) => "Broadcast cancelled.",

        ("admin.broadcast.error", true) => "Ошибка отправки broadcast. Проверьте логи.",
        ("admin.broadcast.error", false) => "Broadcast send failed. Check logs.",

        // =====================================================================
        // Admin assign / status
        // =====================================================================
        ("admin.assign.success", true) => "Тикет назначен на вас.",
        ("admin.assign.success", false) => "Ticket assigned to you.",

        ("admin.status.changed", true) => "Статус тикета изменён.",
        ("admin.status.changed", false) => "Ticket status updated.",

        // Fallback - unknown key
        (_, _) => "???",
    }
}

/// Format version for strings with placeholders - returns String
pub fn tf(lang: Option<&str>, key: &str, args: &[&str]) -> String {
    let template = t(lang, key);
    let mut result = template.to_string();
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{}}}", i), arg);
    }
    result
}
