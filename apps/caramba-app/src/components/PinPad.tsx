import { ReactNode } from 'react';
import './PinPad.css';

type PinPadProps = {
    title: string;
    subtitle?: string;
    valueLength: number;
    error?: string | null;
    busy?: boolean;
    onDigit: (digit: string) => void;
    onBackspace: () => void;
    onClear: () => void;
    footer?: ReactNode;
};

const KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9', 'clear', '0', 'back'];

export default function PinPad({
    title,
    subtitle,
    valueLength,
    error,
    busy = false,
    onDigit,
    onBackspace,
    onClear,
    footer,
}: PinPadProps) {
    const handleKey = (key: string) => {
        if (busy) return;
        if (key === 'clear') {
            onClear();
            return;
        }
        if (key === 'back') {
            onBackspace();
            return;
        }
        onDigit(key);
    };

    return (
        <div className="pinpad">
            <div className="pinpad-header">
                <h2>{title}</h2>
                {subtitle && <p>{subtitle}</p>}
            </div>

            <div className="pinpad-dots" aria-label={`${valueLength} of 4 digits entered`}>
                {[0, 1, 2, 3].map((idx) => (
                    <span
                        key={idx}
                        className={`pin-dot ${idx < valueLength ? 'filled' : ''}`}
                    />
                ))}
            </div>

            {error && <div className="pinpad-error">{error}</div>}

            <div className="pinpad-grid" role="group" aria-label="PIN keypad">
                {KEYS.map((key) => (
                    <button
                        key={key}
                        type="button"
                        className={`pin-key ${key === 'clear' || key === 'back' ? 'action' : ''}`}
                        onClick={() => handleKey(key)}
                        disabled={busy}
                    >
                        {key === 'clear' ? 'Clear' : key === 'back' ? '⌫' : key}
                    </button>
                ))}
            </div>

            {footer && <div className="pinpad-footer">{footer}</div>}
        </div>
    );
}
