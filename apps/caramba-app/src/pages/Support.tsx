import { useEffect, useReducer, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { useAppLock } from '../context/AppLockContext'
import { useAuth } from '../context/AuthContext'
import { normalizePinInput } from '../security/pin'
import PinPad from '../components/PinPad'
import './Support.css'

// Фолбэк на случай, если support_url не задан в настройках панели
const DEFAULT_SUPPORT_URL = 'https://t.me/'

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
    const { t } = useTranslation()
    const navigate = useNavigate()
    const { userStats } = useAuth()
    // URL поддержки: берётся из ответа /api/client/user/stats (поле support_url)
    const supportUrl = userStats?.support_url?.trim() || DEFAULT_SUPPORT_URL
    const { isPinEnabled, lockNow, enablePin, changePin, disablePin, pinUpdatedAt } = useAppLock()
    const [flow, dispatch] = useReducer(flowReducer, flowInitialState)
    const [notice, setNotice] = useState<{ type: 'success' | 'error'; text: string } | null>(null)

    const closeModal = () => dispatch({ type: 'CLOSE' });

    const handleDigit = (digit: string) => dispatch({ type: 'DIGIT', digit });
    const handleBackspace = () => dispatch({ type: 'BACKSPACE' });
    const handleClear = () => dispatch({ type: 'CLEAR' });

    // FAQ-список с переводами через i18n
    const FAQS: Array<{ q: string; a: string }> = t('support.faqItems', { returnObjects: true }) as Array<{ q: string; a: string }>

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
                            dispatch({ type: 'SET_ERROR', error: t('support.pinMismatch') });
                            return;
                        }
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await enablePin(flow.input);
                        setNotice({ type: 'success', text: t('support.pinEnabledSuccess') });
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
                            dispatch({ type: 'SET_ERROR', error: t('support.pinMismatchChange') });
                            return;
                        }
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await changePin(flow.currentPin, flow.input);
                        setNotice({ type: 'success', text: t('support.pinChangedSuccess') });
                        closeModal();
                        return;
                    }
                    case 'disable_verify': {
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await disablePin(flow.input);
                        setNotice({ type: 'success', text: t('support.pinDisabledSuccess') });
                        closeModal();
                        return;
                    }
                    default:
                        return;
                }
            } catch (e: any) {
                const msg = e?.message || t('support.operationFailed')
                dispatch({ type: 'SET_ERROR', error: msg });
                setNotice({ type: 'error', text: msg });
            }
        };

        void run();
    }, [flow, enablePin, changePin, disablePin]);

    // Заголовки и подзаголовки шагов PIN-ввода
    const pinStepTitle: Record<Exclude<PinStep, 'closed'>, string> = {
        setup_new: t('support.pinStepSetupNew'),
        setup_confirm: t('support.pinStepSetupConfirm'),
        change_current: t('support.pinStepChangeCurrent'),
        change_new: t('support.pinStepChangeNew'),
        change_confirm: t('support.pinStepChangeConfirm'),
        disable_verify: t('support.pinStepDisableVerify'),
    };

    const pinStepSubtitle: Record<Exclude<PinStep, 'closed'>, string> = {
        setup_new: t('support.pinSubSetupNew'),
        setup_confirm: t('support.pinSubSetupConfirm'),
        change_current: t('support.pinSubChangeCurrent'),
        change_new: t('support.pinSubChangeNew'),
        change_confirm: t('support.pinSubChangeConfirm'),
        disable_verify: t('support.pinSubDisableVerify'),
    };

    return (
        <div className="page support-page">
            <header className="page-header">
                <h2>{t('support.title')}</h2>
            </header>

            {notice && (
                <div className={`support-notice ${notice.type}`}>
                    {notice.text}
                </div>
            )}

            <button className="contact-hero glass-card" onClick={() => window.open(supportUrl, '_blank')}>
                <span className="contact-icon">TG</span>
                <div>
                    <span className="contact-title">{t('support.contactTitle')}</span>
                    <span className="contact-desc">{t('support.contactDesc')}</span>
                </div>
                <span className="contact-arrow">{'>'}</span>
            </button>

            <section className="support-connect-card glass-card">
                <div>
                    <h3>{t('support.connectionIssueTitle')}</h3>
                    <p>{t('support.connectionIssueDesc')}</p>
                </div>
                <div className="support-connect-actions">
                    <button className="btn-primary" onClick={() => navigate('/')}>
                        {t('support.openCenter')}
                    </button>
                    <button className="btn-secondary" onClick={() => navigate('/support/connect')}>
                        {t('support.hiddifyGuide')}
                    </button>
                </div>
            </section>

            <div className="faq-section">
                <h3>{t('support.faqTitle')}</h3>
                <div className="faq-list">
                    {FAQS.map((faq, i) => (
                        <details key={i} className="faq-item glass-card">
                            <summary>{faq.q}</summary>
                            <p>{faq.a}</p>
                        </details>
                    ))}
                </div>
            </div>

            <div className="security-card security-card-muted glass-card">
                <div className="security-card-head">
                    <div>
                        <h3>{t('support.pinTitle')}</h3>
                        <p>{t('support.pinDesc')}</p>
                    </div>
                    <span className={`security-badge ${isPinEnabled ? 'enabled' : 'disabled'}`}>
                        {isPinEnabled ? t('support.pinEnabled') : t('support.pinDisabled')}
                    </span>
                </div>

                {pinUpdatedAt && (
                    <div className="security-meta">
                        {t('support.pinUpdatedAt', { date: new Date(pinUpdatedAt).toLocaleString() })}
                    </div>
                )}

                <div className="security-actions">
                    {!isPinEnabled && (
                        <button
                            className="btn-secondary"
                            onClick={() => dispatch({ type: 'OPEN', step: 'setup_new' })}
                        >
                            {t('support.enablePin')}
                        </button>
                    )}
                    {isPinEnabled && (
                        <>
                            <button
                                className="btn-secondary"
                                onClick={lockNow}
                            >
                                {t('support.lockNow')}
                            </button>
                            <button
                                className="btn-secondary"
                                onClick={() => dispatch({ type: 'OPEN', step: 'change_current' })}
                            >
                                {t('support.changePin')}
                            </button>
                            <button
                                className="btn-secondary btn-danger-outline"
                                onClick={() => dispatch({ type: 'OPEN', step: 'disable_verify' })}
                            >
                                {t('support.disablePin')}
                            </button>
                        </>
                    )}
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
                                {t('common.cancel')}
                            </button>
                        )}
                    />
                </div>
            )}
        </div>
    )
}
