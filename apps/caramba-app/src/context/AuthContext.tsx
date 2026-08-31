import React, { createContext, useContext, useEffect, useRef, useState } from 'react';
import WebApp from '@twa-dev/sdk';
import { useTranslation } from 'react-i18next';

export interface UserStats {
    traffic_used: number;
    total_traffic: number;
    days_left: number;
    plan_name: string;
    active_subscriptions?: number;
    balance: number;
    total_download: number;
    total_upload: number;
    traffic_limit: number;
    brand_name?: string;
    // URL поддержки из настроек сервера (устанавливается в панели администратора)
    support_url?: string;
    // Username бота (без @) — для кнопки «Открыть чат» после отправки платёжной ссылки
    bot_username?: string;
}

export interface UserSubscription {
    id: number;
    plan_id: number;
    plan_name: string;
    plan_description: string | null;
    status: string;
    used_traffic_bytes: number;
    used_traffic_gb: string;
    /** Лимит ТАРИФА в ГБ (0 = безлимит). Не включает бонусный трафик. */
    traffic_limit_gb: number;
    /**
     * Потолок, по которому панель реально отключает/троттлит: лимит тарифа
     * ПЛЮС бонусный трафик пользователя. `null` = безлимит, `undefined` =
     * старая панель, которая ещё не отдаёт это поле (тогда фолбэк на
     * traffic_limit_gb). См. subscriptionLimitBytes().
     */
    traffic_limit_bytes?: number | null;
    /** Бонусный трафик пользователя в МБ (разовые начисления сверх тарифа). */
    bonus_traffic_mb?: number;
    expires_at: string;
    created_at: string;
    days_left: number;
    duration_days: number;
    note: string | null;
    auto_renew: boolean;
    subscription_uuid: string;
    active_devices?: number;
    device_limit?: number;
    last_node_id?: number | null;
    last_node_name?: string | null;
    last_node_flag?: string | null;
    last_sub_access?: string | null;
    subscription_url: string;
    primary_vless_link?: string | null;
    vless_links?: string[];
    singbox_variants?: SingboxConnectionVariant[];
    /** Бесплатный план — показываем особый UX с ежедневным пополнением */
    is_free?: boolean;
    /** МБ ежедневного пополнения трафика */
    daily_traffic_mb?: number;
}

export interface SingboxConnectionVariant {
    id: string;
    label: string;
    summary: string;
    family: string;
    transport: string;
    relay: boolean;
}

interface User {
    id: number;
    username: string;
    full_name?: string;
    active_subscriptions: number;
    balance?: number;
}

interface AuthContextType {
    isAuthenticated: boolean;
    user: User | null;
    token: string | null;
    userStats: UserStats | null;
    subscriptions: UserSubscription[];
    isLoading: boolean;
    error: string | null;
    refreshData: () => Promise<void>;
}

const AuthContext = createContext<AuthContextType>({
    isAuthenticated: false,
    user: null,
    token: null,
    userStats: null,
    subscriptions: [],
    isLoading: true,
    error: null,
    refreshData: async () => { },
});

export const useAuth = () => useContext(AuthContext);

export const AuthProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    // Ошибки авторизации/загрузки видит пользователь в баннере на Home,
    // Billing и Promo — поэтому они переводятся здесь, а не хранятся кодами.
    const { t } = useTranslation();
    const [user, setUser] = useState<User | null>(null);
    const [token, setToken] = useState<string | null>(localStorage.getItem('jwt_token'));
    const [userStats, setUserStats] = useState<UserStats | null>(null);
    const [subscriptions, setSubscriptions] = useState<UserSubscription[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const fetchWithTimeout = async (input: RequestInfo | URL, init?: RequestInit, timeoutMs = 12000) => {
        const timeoutController = new AbortController();
        const timeout = setTimeout(() => timeoutController.abort(), timeoutMs);
        // Объединяем внешний signal (если есть) с timeout-signal через AbortSignal.any
        // Фоллбэк для браузеров без AbortSignal.any: используем только timeout-signal
        const externalSignal = (init as RequestInit & { signal?: AbortSignal })?.signal;
        const signal = externalSignal && typeof AbortSignal.any === 'function'
            ? AbortSignal.any([timeoutController.signal, externalSignal])
            : timeoutController.signal;
        try {
            return await fetch(input, { ...init, signal });
        } finally {
            clearTimeout(timeout);
        }
    };

    // Обменять свежий Telegram initData на новый JWT. Используется и при первом
    // входе, и для тихой переавторизации, когда сохранённый токен протух:
    // initData у нас всегда под рукой, показывать «Session expired» незачем.
    const reauthInFlight = useRef(false);
    const exchangeInitData = async (): Promise<string | null> => {
        const initData = (WebApp.initData || '').trim();
        if (!initData) return null;
        try {
            const response = await fetchWithTimeout('/api/client/auth/telegram', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ init_data: initData }),
            });
            if (!response.ok) return null;
            const data = await response.json();
            setUser(data.user);
            localStorage.setItem('jwt_token', data.token);
            setToken(data.token);
            return data.token;
        } catch {
            return null;
        }
    };

    // Initial Auth — выполняется один раз при монтировании если нет токена в localStorage
    useEffect(() => {
        if (token) return; // токен уже есть — пропускаем, следующий эффект займётся данными

        const controller = new AbortController();

        const initAuth = async () => {
            try {
                setError(null);
                // Используем только SDK — (window as any) fallback убран: @twa-dev/sdk нормализует initData
                const initData = (WebApp.initData || '').trim();

                if (initData) {
                    const response = await fetchWithTimeout('/api/client/auth/telegram', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ init_data: initData }),
                        signal: controller.signal,
                    });

                    if (response.ok) {
                        const data = await response.json();
                        setToken(data.token);
                        setUser(data.user);
                        localStorage.setItem('jwt_token', data.token);
                        setError(null);
                    } else {
                        // Текст сервера пропускаем мимо перевода только если он
                        // есть: это диагностика для поддержки, а не строка для юзера.
                        const errText = await response.text();
                        setError(errText || t('errors.authFailed', { status: response.status }));
                    }
                } else if (!import.meta.env.DEV) {
                    setError(t('errors.authMissing'));
                } else {
                    // Dev-режим — данных Telegram нет, показываем предупреждение
                    setError(t('errors.devNoInitData'));
                }
            } catch (e: any) {
                if (e?.name === 'AbortError') return; // компонент размонтирован — игнорируем
                setError(e?.name === 'AbortError' ? t('errors.authTimeout') : t('errors.authUnknown'));
            }
        };

        void initAuth();

        return () => { controller.abort(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Fetch Data when token is available — запускаем refreshData при появлении токена
    useEffect(() => {
        if (token) {
            void refreshData();
        } else {
            // Токен так и не появился — снимаем флаг загрузки через небольшую задержку
            const timer = setTimeout(() => setIsLoading(false), 1000);
            return () => clearTimeout(timer);
        }
    // refreshData намеренно не включён в deps — его идентичность стабильна внутри рендера,
    // добавление привело бы к повторным вызовам при каждом обновлении данных.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [token]);

    const refreshData = async () => {
        if (!token) {
            setIsLoading(false);
            return;
        }
        setIsLoading(true);
        try {
            const [statsRes, subsRes] = await Promise.all([
                fetchWithTimeout('/api/client/user/stats', { headers: { Authorization: `Bearer ${token}` } }),
                fetchWithTimeout('/api/client/user/subscriptions', { headers: { Authorization: `Bearer ${token}` } })
            ]);

            if (statsRes.status === 401 || subsRes.status === 401) {
                localStorage.removeItem('jwt_token');
                // Токен протух — тихо меняем initData на новый JWT. setToken
                // перезапустит refreshData через эффект [token]; reauthInFlight
                // страхует от цикла, если сервер отвергает и свежие токены.
                if (!reauthInFlight.current) {
                    reauthInFlight.current = true;
                    const fresh = await exchangeInitData();
                    if (fresh) return; // эффект по token перезапросит данные
                }
                setToken(null);
                setSubscriptions([]);
                setUserStats(null);
                setError(t('errors.sessionExpired'));
                return;
            }
            reauthInFlight.current = false;

            if (statsRes.ok) {
                const s = await statsRes.json();
                setUserStats({
                    ...s,
                    traffic_limit: s.total_traffic || s.traffic_limit || 0,
                    total_download: s.total_download || s.traffic_used || 0,
                    total_upload: s.total_upload || 0,
                });
            }
            // Тихая ошибка stats — данные просто не обновятся, но UX не ломается
            if (subsRes.ok) {
                const data = await subsRes.json();
                // API возвращает массив подписок
                setSubscriptions(Array.isArray(data) ? data : [data]);
            } else {
                setSubscriptions([]);
            }
        } catch (e: any) {
            if (e?.name !== 'AbortError') {
                setError(e?.name === 'AbortError' ? t('errors.dataTimeout') : t('errors.dataFetch'));
            }
        } finally {
            setIsLoading(false);
        }
    };

    return (
        <AuthContext.Provider value={{
            isAuthenticated: !!token,
            user,
            token,
            userStats,
            subscriptions,
            isLoading,
            error,
            refreshData
        }}>
            {children}
        </AuthContext.Provider>
    );
};
