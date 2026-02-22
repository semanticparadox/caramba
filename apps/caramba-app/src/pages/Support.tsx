import { useEffect, useReducer, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAppLock } from '../context/AppLockContext'
import { normalizePinInput } from '../security/pin'
import PinPad from '../components/PinPad'
import './Support.css'

const FAQS = [
    {
        q: "How do I connect?",
        a: "Go to Subscription, copy the link, and paste it into your VPN client (Hiddify, Sing-box, V2Ray, etc)."
    },
    {
        q: "Which server is fastest?",
        a: "Use the Servers page to see distances and find the closest server to your location."
    },
    {
        q: "How do I renew?",
        a: "Your subscription auto-renews if you have balance. Top up in the Billing section."
    },
    {
        q: "What VPN apps can I use?",
        a: "We support Sing-box, V2Ray/Xray, Clash, and Hiddify. Get your config from the Servers page."
    }
]

type PinStep =
    | 'closed'
    | 'setup_new'
    | 'setup_confirm'
    | 'change_current'
    | 'change_new'
    | 'change_confirm'
    | 'disable_verify';

type PinFlowState = {
    step: PinStep;
    input: string;
    firstPin: string;
    currentPin: string;
    error: string | null;
    busy: boolean;
};

type PinFlowAction =
    | { type: 'OPEN'; step: Exclude<PinStep, 'closed'> }
    | { type: 'DIGIT'; digit: string }
    | { type: 'BACKSPACE' }
    | { type: 'CLEAR' }
    | { type: 'SET_STEP'; step: PinStep; firstPin?: string; currentPin?: string }
    | { type: 'SET_ERROR'; error: string | null }
    | { type: 'SET_BUSY'; busy: boolean }
    | { type: 'CLOSE' };

const flowInitialState: PinFlowState = {
    step: 'closed',
    input: '',
    firstPin: '',
    currentPin: '',
    error: null,
    busy: false,
};

function flowReducer(state: PinFlowState, action: PinFlowAction): PinFlowState {
    switch (action.type) {
        case 'OPEN':
            return {
                ...flowInitialState,
                step: action.step,
            };
        case 'DIGIT':
            if (state.input.length >= 4 || state.busy) return state;
            return {
                ...state,
                error: null,
                input: normalizePinInput(`${state.input}${action.digit}`),
            };
        case 'BACKSPACE':
            if (state.busy) return state;
            return { ...state, error: null, input: state.input.slice(0, -1) };
        case 'CLEAR':
            if (state.busy) return state;
            return { ...state, error: null, input: '' };
        case 'SET_STEP':
            return {
                ...state,
                step: action.step,
                input: '',
                error: null,
                busy: false,
                firstPin: action.firstPin ?? state.firstPin,
                currentPin: action.currentPin ?? state.currentPin,
            };
        case 'SET_ERROR':
            return { ...state, error: action.error, busy: false, input: '' };
        case 'SET_BUSY':
            return { ...state, busy: action.busy };
        case 'CLOSE':
            return flowInitialState;
        default:
            return state;
    }
}

export default function Support() {
    const navigate = useNavigate()
    const { isPinEnabled, lockNow, enablePin, changePin, disablePin, pinUpdatedAt } = useAppLock()
    const [flow, dispatch] = useReducer(flowReducer, flowInitialState)
    const [notice, setNotice] = useState<{ type: 'success' | 'error'; text: string } | null>(null)

    const goBack = () => {
        if (window.history.length > 1) {
            navigate(-1)
        } else {
            navigate('/')
        }
    }

    const closeModal = () => dispatch({ type: 'CLOSE' });

    const handleDigit = (digit: string) => dispatch({ type: 'DIGIT', digit });
    const handleBackspace = () => dispatch({ type: 'BACKSPACE' });
    const handleClear = () => dispatch({ type: 'CLEAR' });

    useEffect(() => {
        if (flow.step === 'closed' || flow.input.length !== 4 || flow.busy) return;

        const run = async () => {
            try {
                switch (flow.step) {
                    case 'setup_new': {
                        dispatch({ type: 'SET_STEP', step: 'setup_confirm', firstPin: flow.input });
                        return;
                    }
                    case 'setup_confirm': {
                        if (flow.input !== flow.firstPin) {
                            dispatch({ type: 'SET_STEP', step: 'setup_new' });
                            dispatch({ type: 'SET_ERROR', error: 'PIN mismatch. Re-enter new PIN.' });
                            return;
                        }
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await enablePin(flow.input);
                        setNotice({ type: 'success', text: 'PIN lock enabled.' });
                        closeModal();
                        return;
                    }
                    case 'change_current': {
                        dispatch({
                            type: 'SET_STEP',
                            step: 'change_new',
                            currentPin: flow.input,
                        });
                        return;
                    }
                    case 'change_new': {
                        dispatch({
                            type: 'SET_STEP',
                            step: 'change_confirm',
                            firstPin: flow.input,
                            currentPin: flow.currentPin,
                        });
                        return;
                    }
                    case 'change_confirm': {
                        if (flow.input !== flow.firstPin) {
                            dispatch({
                                type: 'SET_STEP',
                                step: 'change_new',
                                currentPin: flow.currentPin,
                            });
                            dispatch({ type: 'SET_ERROR', error: 'PIN mismatch. Enter new PIN again.' });
                            return;
                        }
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await changePin(flow.currentPin, flow.input);
                        setNotice({ type: 'success', text: 'PIN changed successfully.' });
                        closeModal();
                        return;
                    }
                    case 'disable_verify': {
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await disablePin(flow.input);
                        setNotice({ type: 'success', text: 'PIN lock disabled.' });
                        closeModal();
                        return;
                    }
                    default:
                        return;
                }
            } catch (e: any) {
                dispatch({
                    type: 'SET_ERROR',
                    error: e?.message || 'Operation failed. Please try again.',
                });
                setNotice({
                    type: 'error',
                    text: e?.message || 'Operation failed. Please try again.',
                });
            }
        };

        void run();
    }, [flow, enablePin, changePin, disablePin]);

    const pinStepTitle: Record<Exclude<PinStep, 'closed'>, string> = {
        setup_new: 'Set New PIN',
        setup_confirm: 'Confirm New PIN',
        change_current: 'Enter Current PIN',
        change_new: 'Enter New PIN',
        change_confirm: 'Confirm New PIN',
        disable_verify: 'Disable PIN Lock',
    };

    const pinStepSubtitle: Record<Exclude<PinStep, 'closed'>, string> = {
        setup_new: 'Choose 4 digits to protect Mini App.',
        setup_confirm: 'Re-enter same 4 digits.',
        change_current: 'Verify current PIN first.',
        change_new: 'Set a new 4-digit code.',
        change_confirm: 'Re-enter new code.',
        disable_verify: 'Enter current PIN to turn lock off.',
    };

    return (
        <div className="page support-page">
            <header className="page-header">
                <button className="back-button" onClick={goBack}>←</button>
                <h2>Support</h2>
            </header>

            {notice && (
                <div className={`support-notice ${notice.type}`}>
                    {notice.text}
                </div>
            )}

            <button className="contact-hero glass-card" onClick={() => window.open('https://t.me/SupportBot', '_blank')}>
                <span className="contact-icon">💬</span>
                <div>
                    <span className="contact-title">Chat with Support</span>
                    <span className="contact-desc">Get help from our team</span>
                </div>
                <span className="contact-arrow">→</span>
            </button>

            <div className="security-card glass-card">
                <div className="security-card-head">
                    <div>
                        <h3>Mini App Lock</h3>
                        <p>
                            Protect this Mini App with a 4-digit PIN.
                        </p>
                    </div>
                    <span className={`security-badge ${isPinEnabled ? 'enabled' : 'disabled'}`}>
                        {isPinEnabled ? 'Enabled' : 'Disabled'}
                    </span>
                </div>

                {pinUpdatedAt && (
                    <div className="security-meta">
                        Last updated: {new Date(pinUpdatedAt).toLocaleString()}
                    </div>
                )}

                <div className="security-actions">
                    {!isPinEnabled && (
                        <button
                            className="btn-primary"
                            onClick={() => dispatch({ type: 'OPEN', step: 'setup_new' })}
                        >
                            Enable 4-digit PIN
                        </button>
                    )}
                    {isPinEnabled && (
                        <>
                            <button
                                className="btn-secondary"
                                onClick={lockNow}
                            >
                                Lock Mini App Now
                            </button>
                            <button
                                className="btn-secondary"
                                onClick={() => dispatch({ type: 'OPEN', step: 'change_current' })}
                            >
                                Change PIN
                            </button>
                            <button
                                className="btn-secondary btn-danger-outline"
                                onClick={() => dispatch({ type: 'OPEN', step: 'disable_verify' })}
                            >
                                Disable PIN Lock
                            </button>
                        </>
                    )}
                </div>
            </div>

            <div className="faq-section">
                <h3>FAQ</h3>
                <div className="faq-list">
                    {FAQS.map((faq, i) => (
                        <details key={i} className="faq-item glass-card">
                            <summary>{faq.q}</summary>
                            <p>{faq.a}</p>
                        </details>
                    ))}
                </div>
            </div>

            {flow.step !== 'closed' && (
                <div className="modal-overlay">
                    <PinPad
                        title={pinStepTitle[flow.step]}
                        subtitle={pinStepSubtitle[flow.step]}
                        valueLength={flow.input.length}
                        error={flow.error}
                        busy={flow.busy}
                        onDigit={handleDigit}
                        onBackspace={handleBackspace}
                        onClear={handleClear}
                        footer={(
                            <button
                                type="button"
                                className="btn-secondary"
                                onClick={closeModal}
                                disabled={flow.busy}
                            >
                                Cancel
                            </button>
                        )}
                    />
                </div>
            )}
        </div>
    )
}
