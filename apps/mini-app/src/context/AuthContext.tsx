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
    traffic_limit: number;  // alias for total_traffic
}

export interface UserSubscription {
    uuid: string;
    subscription_url: string;
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
    subscription: UserSubscription | null;
    isLoading: boolean;
    error: string | null;
    refreshData: () => Promise<void>;
}

const AuthContext = createContext<AuthContextType>({
    isAuthenticated: false,
    user: null,
    token: null,
    userStats: null,
    subscription: null,
    isLoading: true,
    error: null,
    refreshData: async () => { },
});

export const useAuth = () => useContext(AuthContext);

export const AuthProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [user, setUser] = useState<User | null>(null);
    const [token, setToken] = useState<string | null>(localStorage.getItem('jwt_token'));
    const [userStats, setUserStats] = useState<UserStats | null>(null);
    const [subscription, setSubscription] = useState<UserSubscription | null>(null);
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    // Initial Auth
    useEffect(() => {
        const initAuth = async () => {
            try {
                // Expand Telegram WebApp
                WebApp.expand();
                const initData = WebApp.initData;

                if (initData) {
                    const response = await fetch('/api/client/auth/telegram', {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ init_data: initData }),
                    });

                    if (response.ok) {
                        const data = await response.json();
                        setToken(data.token);
                        setUser(data.user);
                        localStorage.setItem('jwt_token', data.token);
                    } else {
                        const errText = await response.text();
                        console.error("Auth failed:", errText);
                        setError(errText);
                    }
                } else if (!import.meta.env.DEV) {
                    console.warn("No initData found");
                } else {
                    console.warn("Dev mode — no Telegram initData");
                }
            } catch (e: any) {
                console.error("Auth error:", e);
                setError(e.message);
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
            // Wait for auth to complete or fail
            const timer = setTimeout(() => setIsLoading(false), 1000);
            return () => clearTimeout(timer);
        }
    }, [token]);

    const refreshData = async () => {
        if (!token) return;
        setIsLoading(true);
        try {
            const [statsRes, subRes] = await Promise.all([
                fetch('/api/client/user/stats', { headers: { Authorization: `Bearer ${token}` } }),
                fetch('/api/client/user/subscription', { headers: { Authorization: `Bearer ${token}` } })
            ]);

            if (statsRes.ok) {
                const s = await statsRes.json();
                setUserStats({
                    ...s,
                    traffic_limit: s.total_traffic || s.traffic_limit || 0,
                    total_download: s.total_download || s.traffic_used || 0,
                    total_upload: s.total_upload || 0,
                });
            }
            if (subRes.ok) {
                setSubscription(await subRes.json());
            }
        } catch (e: any) {
            console.error("Data fetch error:", e);
            setError(e.message);
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
            subscription,
            isLoading,
            error,
            refreshData
        }}>
            {children}
        </AuthContext.Provider>
    );
};
