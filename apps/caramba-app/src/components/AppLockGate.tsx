import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAppLock } from '../context/AppLockContext';
import { normalizePinInput } from '../security/pin';
import PinPad from './PinPad';
import './AppLockGate.css';

// Коды ошибок из AppLockContext / pin.ts → переводимые ключи
const PIN_ERROR_KEYS: Record<string, string> = {
    'pin.incorrect': 'applock.errorIncorrect',
    'pin.incorrectCurrent': 'applock.errorIncorrect',
    'pin.incorrectFormat': 'applock.errorIncorrect',
    'pin.enableFailed': 'applock.errorEnableFailed',
    'pin.changeFailed': 'applock.errorChangeFailed',
    'pin.disableFailed': 'applock.errorDisableFailed',
}

export default function AppLockGate() {
    const { t } = useTranslation();
    const { ready, isPinEnabled, isLocked, isBusy, error, unlock, clearError } = useAppLock();

    // Переводим код ошибки если это i18n-ключ, иначе показываем как есть
    const translatedError = error
        ? (PIN_ERROR_KEYS[error] ? t(PIN_ERROR_KEYS[error]) : error)
        : null;
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
        <div className="applock-overlay" role="dialog" aria-modal="true" aria-label={t('applock.locked')}>
            <div className="applock-brand">
                <span className="applock-logo">PIN</span>
                <h1>{t('applock.locked')}</h1>
                <p>{t('applock.enterPin')}</p>
            </div>
            <PinPad
                title={t('applock.unlock')}
                subtitle={t('applock.unlockSubtitle')}
                valueLength={pin.length}
                error={translatedError}
                busy={isBusy}
                onDigit={onDigit}
                onBackspace={onBackspace}
                onClear={onClear}
            />
        </div>
    );
}
