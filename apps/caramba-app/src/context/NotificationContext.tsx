import React, { createContext, useContext, useEffect, useRef, useState } from 'react';
import { apiUrl } from '../config';
import { useAuth } from './AuthContext';

interface NotificationContextType {
    unreadCount: number;
    refreshUnreadCount: () => void;
}

const NotificationContext = createContext<NotificationContextType>({
    unreadCount: 0,
    refreshUnreadCount: () => { },
});

export const useNotifications = () => useContext(NotificationContext);

// Опрашиваем /api/client/notifications/unread-count каждые 60 секунд
const POLL_INTERVAL_MS = 60_000;

export const NotificationProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const { token } = useAuth();
    const [unreadCount, setUnreadCount] = useState(0);
    // Используем ref для принудительного обновления извне
    const [refreshTick, setRefreshTick] = useState(0);
    const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

    const refreshUnreadCount = () => setRefreshTick((n) => n + 1);

    useEffect(() => {
        if (!token) {
            setUnreadCount(0);
            return;
        }

        const controller = new AbortController();

        const fetchCount = async () => {
            try {
                const res = await fetch(apiUrl('/api/client/notifications/unread-count'), {
                    headers: { Authorization: `Bearer ${token}` },
                    signal: controller.signal,
                });
                if (res.ok) {
                    const data = await res.json();
                    setUnreadCount(typeof data?.count === 'number' ? data.count : 0);
                }
            } catch {
                // Тихая ошибка — не ломаем UX из-за счётчика уведомлений
            }
        };

        void fetchCount();

        timerRef.current = setInterval(() => {
            void fetchCount();
        }, POLL_INTERVAL_MS);

        return () => {
            controller.abort();
            if (timerRef.current) clearInterval(timerRef.current);
        };
    }, [token, refreshTick]);

    return (
        <NotificationContext.Provider value={{ unreadCount, refreshUnreadCount }}>
            {children}
        </NotificationContext.Provider>
    );
};
