import { useEffect, useReducer, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAppLock } from '../context/AppLockContext'
import { normalizePinInput } from '../security/pin'
import PinPad from '../components/PinPad'
import './Support.css'

const FAQS = [
    {
        q: 'Как подключиться?',
        a: 'Откройте раздел "Мои сервисы", скопируйте ссылку и импортируйте ее в VPN-клиент (Hiddify, Sing-box, V2Ray и т.д.).',
    },
    {
        q: 'Какой сервер самый быстрый?',
        a: 'Используйте экран оптимизации соединения или запустите магическую оптимизацию для автоматического выбора узла.',
    },
    {
        q: 'Как продлить подписку?',
        a: 'Подписка продлевается при наличии средств на балансе. Пополните счет в разделе тарифов/оплаты.',
    },
    {
        q: 'Какие приложения поддерживаются?',
        a: 'Поддерживаются Sing-box, V2Ray/Xray, Clash и Hiddify. Проверенный каталог доступен в разделе "Как подключиться".',
    },
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
                            dispatch({ type: 'SET_ERROR', error: 'PIN не совпадает. Введите новый PIN заново.' });
                            return;
                        }
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await enablePin(flow.input);
                        setNotice({ type: 'success', text: 'PIN-защита включена.' });
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
                            dispatch({ type: 'SET_ERROR', error: 'PIN не совпадает. Введите новый PIN еще раз.' });
                            return;
                        }
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await changePin(flow.currentPin, flow.input);
                        setNotice({ type: 'success', text: 'PIN успешно изменен.' });
                        closeModal();
                        return;
                    }
                    case 'disable_verify': {
                        dispatch({ type: 'SET_BUSY', busy: true });
                        await disablePin(flow.input);
                        setNotice({ type: 'success', text: 'PIN-защита отключена.' });
                        closeModal();
                        return;
                    }
                    default:
                        return;
                }
            } catch (e: any) {
                dispatch({
                    type: 'SET_ERROR',
                    error: e?.message || 'Операция не выполнена. Попробуйте снова.',
                });
                setNotice({
                    type: 'error',
                    text: e?.message || 'Операция не выполнена. Попробуйте снова.',
                });
            }
        };

        void run();
    }, [flow, enablePin, changePin, disablePin]);

    const pinStepTitle: Record<Exclude<PinStep, 'closed'>, string> = {
        setup_new: 'Новый PIN',
        setup_confirm: 'Подтвердите PIN',
        change_current: 'Текущий PIN',
        change_new: 'Новый PIN',
        change_confirm: 'Подтвердите PIN',
        disable_verify: 'Отключение PIN',
    };

    const pinStepSubtitle: Record<Exclude<PinStep, 'closed'>, string> = {
        setup_new: 'Выберите 4 цифры для защиты Mini App.',
        setup_confirm: 'Повторите те же 4 цифры.',
        change_current: 'Сначала подтвердите текущий PIN.',
        change_new: 'Введите новый 4-значный код.',
        change_confirm: 'Повторите новый код.',
        disable_verify: 'Введите текущий PIN для отключения.',
    };

    return (
        <div className="page support-page">
            <header className="page-header">
                <button className="back-button" onClick={goBack}>{'<'}</button>
                <h2>Поддержка</h2>
            </header>

            {notice && (
                <div className={`support-notice ${notice.type}`}>
                    {notice.text}
                </div>
            )}

            <button className="contact-hero glass-card" onClick={() => window.open('https://t.me/SupportBot', '_blank')}>
                <span className="contact-icon">TG</span>
                <div>
                    <span className="contact-title">Написать в поддержку</span>
                    <span className="contact-desc">Ответ команды и разбор проблем</span>
                </div>
                <span className="contact-arrow">{'>'}</span>
            </button>

            <button className="btn-secondary" onClick={() => navigate('/support/connect')}>
                Как подключиться: гид и каталог приложений
            </button>

            <div className="security-card glass-card">
                <div className="security-card-head">
                    <div>
                        <h3>Блокировка Mini App</h3>
                        <p>
                            Защитите Mini App с помощью 4-значного PIN.
                        </p>
                    </div>
                    <span className={`security-badge ${isPinEnabled ? 'enabled' : 'disabled'}`}>
                        {isPinEnabled ? 'Включена' : 'Отключена'}
                    </span>
                </div>

                {pinUpdatedAt && (
                    <div className="security-meta">
                        Обновлено: {new Date(pinUpdatedAt).toLocaleString()}
                    </div>
                )}

                <div className="security-actions">
                    {!isPinEnabled && (
                        <button
                            className="btn-primary"
                            onClick={() => dispatch({ type: 'OPEN', step: 'setup_new' })}
                        >
                            Включить PIN (4 цифры)
                        </button>
                    )}
                    {isPinEnabled && (
                        <>
                            <button
                                className="btn-secondary"
                                onClick={lockNow}
                            >
                                Заблокировать сейчас
                            </button>
                            <button
                                className="btn-secondary"
                                onClick={() => dispatch({ type: 'OPEN', step: 'change_current' })}
                            >
                                Изменить PIN
                            </button>
                            <button
                                className="btn-secondary btn-danger-outline"
                                onClick={() => dispatch({ type: 'OPEN', step: 'disable_verify' })}
                            >
                                Отключить PIN-защиту
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
                                Отмена
                            </button>
                        )}
                    />
                </div>
            )}
        </div>
    )
}
