import React, { createContext, useContext, useEffect, useState } from 'react';
import WebApp from '@twa-dev/sdk';

export interface UserStats {
    traffic_used: number;
    total_traffic: number;
    days_left: number;
    plan_name: string;
    balance: number;
    total_download: number;
    total_upload: number;
    traffic_limit: number;
}

export interface UserSubscription {
    id: number;
    plan_name: string;
    plan_description: string | null;
    status: string;
    used_traffic_bytes: number;
    used_traffic_gb: string;
    traffic_limit_gb: number;
    expires_at: string;
    created_at: string;
    days_left: number;
    duration_days: number;
    note: string | null;
    auto_renew: boolean;
    is_trial: boolean;
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
}

interface User {
    id: number;
    username: string;
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
    const [user, setUser] = useState<User | null>(null);
    const [token, setToken] = useState<string | null>(localStorage.getItem('jwt_token'));
    const [userStats, setUserStats] = useState<UserStats | null>(null);
    const [subscriptions, setSubscriptions] = useState<UserSubscription[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const fetchWithTimeout = async (input: RequestInfo | URL, init?: RequestInit, timeoutMs = 12000) => {
        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), timeoutMs);
        try {
            return await fetch(input, { ...init, signal: controller.signal });
        } finally {
            clearTimeout(timeout);
        }
    };

    // Initial Auth
    useEffect(() => {
        const initAuth = async () => {
            try {
                setError(null);
                WebApp.ready();
                WebApp.expand();
                let initData = (WebApp.initData || '').trim();
                if (!initData) {
                    const fallbackData = (window as any)?.Telegram?.WebApp?.initData;
                    if (typeof fallbackData === 'string') {
                        initData = fallbackData.trim();
                    }
                }

                if (initData) {
                    const response = await fetchWithTimeout('/api/client/auth/telegram', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ init_data: initData }),
                    });

                    if (response.ok) {
                        const data = await response.json();
                        setToken(data.token);
                        setUser(data.user);
                        localStorage.setItem('jwt_token', data.token);
                        setError(null);
                    } else {
                        const errText = await response.text();
                        console.error("Auth failed:", errText);
                        setError(errText || `Auth failed (${response.status})`);
                    }
                } else if (!import.meta.env.DEV) {
                    console.warn("No initData found");
                    setError('Telegram auth data is missing. Reopen Mini App from bot.');
                } else {
                    console.warn("Dev mode — no Telegram initData");
                    setError('Dev mode: no Telegram initData');
                }
            } catch (e: any) {
                console.error("Auth error:", e);
                setError(e?.name === 'AbortError' ? 'Auth request timed out' : e.message);
            }
        };

        if (!token) {
            initAuth();
        }
    }, []);

    // Fetch Data when token is available
    useEffect(() => {
        if (token) {
            refreshData();
        } else {
            const timer = setTimeout(() => setIsLoading(false), 1000);
            return () => clearTimeout(timer);
        }
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
                setToken(null);
                setSubscriptions([]);
                setUserStats(null);
                setError('Session expired. Reopen Mini App from bot.');
                return;
            }

            if (statsRes.ok) {
                const s = await statsRes.json();
                setUserStats({
                    ...s,
                    traffic_limit: s.total_traffic || s.traffic_limit || 0,
                    total_download: s.total_download || s.traffic_used || 0,
                    total_upload: s.total_upload || 0,
                });
            } else {
                console.warn("Stats fetch failed:", statsRes.status, await statsRes.text().catch(() => ''));
            }
            if (subsRes.ok) {
                const data = await subsRes.json();
                console.log("Subscriptions fetched:", data?.length || 0, "items");
                // API now returns an array
                setSubscriptions(Array.isArray(data) ? data : [data]);
            } else {
                console.error("Subscriptions fetch failed:", subsRes.status, await subsRes.text().catch(() => ''));
                setSubscriptions([]);
            }
        } catch (e: any) {
            console.error("Data fetch error:", e);
            setError(e?.name === 'AbortError' ? 'Data request timed out' : e.message);
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
