import React, { createContext, useContext, useEffect, useMemo, useReducer } from 'react';
import {
    changePin as changeStoredPin,
    disablePin as disableStoredPin,
    getPinStorageMeta,
    hasPinConfigured,
    isPinSessionUnlocked,
    markPinSessionUnlocked,
    setPin as setStoredPin,
    verifyPin as verifyStoredPin,
} from '../security/pin';

type AppLockState = {
    ready: boolean;
    isPinEnabled: boolean;
    isLocked: boolean;
    isBusy: boolean;
    error: string | null;
    failedAttempts: number;
    pinUpdatedAt: number | null;
};

type AppLockAction =
    | { type: 'INIT'; enabled: boolean; locked: boolean; pinUpdatedAt: number | null }
    | { type: 'SET_BUSY'; busy: boolean }
    | { type: 'UNLOCK_SUCCESS' }
    | { type: 'UNLOCK_FAILED'; message: string }
    | { type: 'PIN_ENABLED'; pinUpdatedAt: number | null }
    | { type: 'PIN_DISABLED' }
    | { type: 'LOCK_NOW' }
    | { type: 'CLEAR_ERROR' };

type AppLockContextType = AppLockState & {
    unlock: (pin: string) => Promise<boolean>;
    lockNow: () => void;
    enablePin: (pin: string) => Promise<void>;
    changePin: (currentPin: string, newPin: string) => Promise<void>;
    disablePin: (currentPin: string) => Promise<void>;
    clearError: () => void;
};

const initialState: AppLockState = {
    ready: false,
    isPinEnabled: false,
    isLocked: false,
    isBusy: false,
    error: null,
    failedAttempts: 0,
    pinUpdatedAt: null,
};

function reducer(state: AppLockState, action: AppLockAction): AppLockState {
    switch (action.type) {
        case 'INIT':
            return {
                ...state,
                ready: true,
                isPinEnabled: action.enabled,
                isLocked: action.locked,
                isBusy: false,
                error: null,
                failedAttempts: 0,
                pinUpdatedAt: action.pinUpdatedAt,
            };
        case 'SET_BUSY':
            return { ...state, isBusy: action.busy };
        case 'UNLOCK_SUCCESS':
            return { ...state, isLocked: false, error: null, failedAttempts: 0, isBusy: false };
        case 'UNLOCK_FAILED':
            return {
                ...state,
                isLocked: true,
                isBusy: false,
                error: action.message,
                failedAttempts: state.failedAttempts + 1,
            };
        case 'PIN_ENABLED':
            return {
                ...state,
                isPinEnabled: true,
                isLocked: false,
                isBusy: false,
                error: null,
                failedAttempts: 0,
                pinUpdatedAt: action.pinUpdatedAt,
            };
        case 'PIN_DISABLED':
            return {
                ...state,
                isPinEnabled: false,
                isLocked: false,
                isBusy: false,
                error: null,
                failedAttempts: 0,
                pinUpdatedAt: null,
            };
        case 'LOCK_NOW':
            return {
                ...state,
                isLocked: state.isPinEnabled,
                error: null,
                failedAttempts: 0,
            };
        case 'CLEAR_ERROR':
            return { ...state, error: null };
        default:
            return state;
    }
}

const AppLockContext = createContext<AppLockContextType | null>(null);

export const AppLockProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [state, dispatch] = useReducer(reducer, initialState);

    useEffect(() => {
        const enabled = hasPinConfigured();
        const meta = getPinStorageMeta();
        const locked = enabled && !isPinSessionUnlocked();
        dispatch({
            type: 'INIT',
            enabled,
            locked,
            pinUpdatedAt: meta.updatedAt,
        });
    }, []);

    const api = useMemo<AppLockContextType>(() => {
        const unlock = async (pin: string) => {
            dispatch({ type: 'SET_BUSY', busy: true });
            const ok = await verifyStoredPin(pin);
            if (ok) {
                markPinSessionUnlocked(true);
                dispatch({ type: 'UNLOCK_SUCCESS' });
                return true;
            }
            dispatch({ type: 'UNLOCK_FAILED', message: 'Incorrect PIN. Try again.' });
            return false;
        };

        const lockNow = () => {
            markPinSessionUnlocked(false);
            dispatch({ type: 'LOCK_NOW' });
        };

        const enablePin = async (pin: string) => {
            dispatch({ type: 'SET_BUSY', busy: true });
            try {
                await setStoredPin(pin);
                const meta = getPinStorageMeta();
                dispatch({ type: 'PIN_ENABLED', pinUpdatedAt: meta.updatedAt });
            } catch (e: any) {
                dispatch({
                    type: 'UNLOCK_FAILED',
                    message: e?.message || 'Failed to enable PIN.',
                });
                throw e;
            }
        };

        const changePin = async (currentPin: string, newPin: string) => {
            dispatch({ type: 'SET_BUSY', busy: true });
            try {
                await changeStoredPin(currentPin, newPin);
                const meta = getPinStorageMeta();
                dispatch({ type: 'PIN_ENABLED', pinUpdatedAt: meta.updatedAt });
            } catch (e: any) {
                dispatch({
                    type: 'UNLOCK_FAILED',
                    message: e?.message || 'Failed to change PIN.',
                });
                throw e;
            }
        };

        const disablePin = async (currentPin: string) => {
            dispatch({ type: 'SET_BUSY', busy: true });
            try {
                await disableStoredPin(currentPin);
                dispatch({ type: 'PIN_DISABLED' });
            } catch (e: any) {
                dispatch({
                    type: 'UNLOCK_FAILED',
                    message: e?.message || 'Failed to disable PIN.',
                });
                throw e;
            }
        };

        const clearError = () => dispatch({ type: 'CLEAR_ERROR' });

        return {
            ...state,
            unlock,
            lockNow,
            enablePin,
            changePin,
            disablePin,
            clearError,
        };
    }, [state]);

    return <AppLockContext.Provider value={api}>{children}</AppLockContext.Provider>;
};

export function useAppLock(): AppLockContextType {
    const ctx = useContext(AppLockContext);
    if (!ctx) {
        throw new Error('useAppLock must be used inside AppLockProvider');
    }
    return ctx;
}
