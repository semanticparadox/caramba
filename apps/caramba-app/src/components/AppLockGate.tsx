import { useEffect, useState } from 'react';
import { useAppLock } from '../context/AppLockContext';
import { normalizePinInput } from '../security/pin';
import PinPad from './PinPad';
import './AppLockGate.css';

export default function AppLockGate() {
    const { ready, isPinEnabled, isLocked, isBusy, error, unlock, clearError } = useAppLock();
    const [pin, setPin] = useState('');

    useEffect(() => {
        if (!isLocked) {
            setPin('');
        }
    }, [isLocked]);

    const onDigit = (digit: string) => {
        clearError();
        if (pin.length >= 4) return;
        setPin((prev) => normalizePinInput(`${prev}${digit}`));
    };

    const onBackspace = () => {
        clearError();
        setPin((prev) => prev.slice(0, -1));
    };

    const onClear = () => {
        clearError();
        setPin('');
    };

    useEffect(() => {
        if (pin.length !== 4 || !isLocked || isBusy) return;
        void unlock(pin).then((ok) => {
            if (!ok) setPin('');
        });
    }, [pin, isLocked, isBusy, unlock]);

    if (!ready || !isPinEnabled || !isLocked) return null;

    return (
        <div className="applock-overlay" role="dialog" aria-modal="true" aria-label="Mini App lock">
            <div className="applock-brand">
                <span className="applock-logo">🔒</span>
                <h1>Mini App Locked</h1>
                <p>Enter your 4-digit PIN to continue.</p>
            </div>
            <PinPad
                title="Unlock"
                subtitle="For your privacy this app is protected with PIN."
                valueLength={pin.length}
                error={error}
                busy={isBusy}
                onDigit={onDigit}
                onBackspace={onBackspace}
                onClear={onClear}
            />
        </div>
    );
}
