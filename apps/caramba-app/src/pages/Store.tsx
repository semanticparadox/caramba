import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { apiUrl } from '../config'
import { useAuth } from '../context/AuthContext'
import BotPaymentPanel from '../components/BotPaymentPanel'
import DrawerModal from '../components/DrawerModal'
import { mapProviderCards } from '../lib/paymentProviders'
import { useBotPayment } from '../lib/useBotPayment'
import './Store.css'

interface Category {
    id: number
    name: string
    description: string | null
}

interface Product {
    id: number
    name: string
    description: string | null
    price: number
    price_raw: number
    product_type: string
}

interface CartItem {
    id: number
    product_id: number
    product_name: string
    quantity: number
    price: number
    total: number
}

interface PaymentProvider {
    id: string
    label: string
}

export default function Store() {
    const navigate = useNavigate()
    const { t } = useTranslation()
    const { token, error, userStats, refreshData } = useAuth()
    const [categories, setCategories] = useState<Category[]>([])
    const [products, setProducts] = useState<Product[]>([])
    const [cart, setCart] = useState<CartItem[]>([])
    const [providers, setProviders] = useState<PaymentProvider[]>([])
    const [activeCat, setActiveCat] = useState<number | null>(null)
    const [loading, setLoading] = useState(true)
    const [showCart, setShowCart] = useState(false)
    const [showPayModal, setShowPayModal] = useState(false)
    const [pendingOrderId, setPendingOrderId] = useState<number | null>(null)
    const [checkoutMsg, setCheckoutMsg] = useState('')
    // Ручная оплата: ссылка на завершение оплаты (рендерится тапабельной, не текстом).
    const [manualUrl, setManualUrl] = useState<string | null>(null)
    const [addedId, setAddedId] = useState<number | null>(null)

    // Внешний чекаут: ссылка ушла в чат бота, статус сессии поллится до completed.
    const { botPayment, startBotPayment, clearBotPayment } = useBotPayment({
        token,
        onRefresh: refreshData,
    })

    const providerCards = mapProviderCards(providers)

    const headers = { Authorization: `Bearer ${token}` }

    useEffect(() => {
        if (!token) {
            setLoading(false)
            return
        }

        const controller = new AbortController()
        const signal = controller.signal

        Promise.all([
            fetch(apiUrl('/api/client/store/categories'), { headers, signal }).then(r => { if (!r.ok) throw new Error(`categories: ${r.status}`); return r.json() }),
            fetch(apiUrl('/api/client/payment/providers'), { headers, signal }).then(r => { if (!r.ok) throw new Error(`providers: ${r.status}`); return r.json() }),
        ])
            .then(([cats, pays]) => {
                if (signal.aborted) return
                setCategories(cats)
                setProviders(pays.providers || [])
                if (cats.length > 0) {
                    setActiveCat(cats[0].id)
                    void loadProducts(cats[0].id)
                }
            })
            .catch(e => { if (!signal.aborted) console.error(e) })
            .finally(() => { if (!signal.aborted) setLoading(false) })

        return () => controller.abort()
    }, [token])

    const goBack = () => {
        if (window.history.length > 1) {
            navigate(-1)
        } else {
            navigate('/')
        }
    }

    const loadProducts = async (catId: number) => {
        setActiveCat(catId)
        try {
            const res = await fetch(apiUrl(`/api/client/store/products/${catId}`), { headers })
            if (res.ok) setProducts(await res.json())
        } catch (e) { console.error(e) }
    }

    const loadCart = async () => {
        try {
            const res = await fetch(apiUrl('/api/client/store/cart'), { headers })
            if (res.ok) setCart(await res.json())
        } catch (e) { console.error(e) }
    }

    const addToCart = async (productId: number) => {
        try {
            const res = await fetch(apiUrl('/api/client/store/cart/add'), {
                method: 'POST',
                headers: { ...headers, 'Content-Type': 'application/json' },
                body: JSON.stringify({ product_id: productId, quantity: 1 }),
            })
            if (res.ok) {
                setAddedId(productId)
                setTimeout(() => setAddedId(null), 1500)
                await loadCart()
            }
        } catch (e) { console.error(e) }
    }

    const checkout = async () => {
        setCheckoutMsg('')
        try {
            const res = await fetch(apiUrl('/api/client/store/checkout'), {
                method: 'POST',
                headers,
            })
            if (res.ok) {
                const data = await res.json()
                setPendingOrderId(data.order_id)
                setShowCart(false)
                setShowPayModal(true)
                setCart([])
            } else {
                const err = await res.text()
                setCheckoutMsg(err || 'Не удалось оформить заказ')
            }
        } catch (e) { setCheckoutMsg('Сетевая ошибка при оформлении заказа') }
    }

    const handleProviderSelect = async (providerId: string) => {
        if (!pendingOrderId) return

        try {
            const res = await fetch(apiUrl('/api/client/payment/invoice'), {
                method: 'POST',
                headers: { ...headers, 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    order_id: pendingOrderId,
                    provider: providerId,
                }),
            })

            if (res.ok) {
                const data = await res.json()
                if (data.invoice_url) {
                    const url = String(data.invoice_url)
                    if (providerId === 'manual') {
                        // Сервер отдаёт относительный путь ("/manual-upload") —
                        // резолвим его против origin панели (apiUrl), иначе при
                        // деплое Mini App на другом origin ссылка вела бы в 404.
                        setManualUrl(/^https?:\/\//i.test(url) ? url : apiUrl(url.startsWith('/') ? url : `/${url}`))
                    } else if (/^https?:\/\//i.test(url)) {
                        // Внешний чекаут: сервер уже отправил ссылку в чат бота
                        // (delivered_via: "bot") — показываем панель и поллим
                        // статус сессии. Никакого window.location.href.
                        startBotPayment(url, data.session_id ? String(data.session_id) : null)
                    } else {
                        // Мгновенное завершение (например, balance возвращает
                        // сентинел "SUCCESS") — оплата уже проведена сервером.
                        setCheckoutMsg(t('payment.confirmed'))
                        void refreshData()
                    }
                }
                setShowPayModal(false)
                setPendingOrderId(null)
            } else {
                const err = await res.text()
                setCheckoutMsg(`Ошибка оплаты: ${err}`)
            }
        } catch (e) {
            setCheckoutMsg('Сетевая ошибка во время оплаты')
        }
    }

    const openCart = async () => {
        await loadCart()
        setShowCart(true)
    }

    const cartTotal = cart.reduce((s, i) => s + i.total, 0)

    if (loading) return <div className="page"><div className="loading">Загрузка магазина...</div></div>

    return (
        <div className="page store-page">
            <header className="page-header">
                <button className="back-button" onClick={goBack}>{'<'}</button>
                <h2>Магазин</h2>
                <button className="cart-toggle" onClick={openCart}>
                    Корзина {cart.length > 0 && <span className="cart-count">{cart.length}</span>}
                </button>
            </header>

            {/* Ссылка на оплату ушла в чат бота — панель со статусом и кнопками */}
            {botPayment && (
                <BotPaymentPanel
                    payment={botPayment}
                    botUsername={userStats?.bot_username}
                    onClose={clearBotPayment}
                />
            )}

            {/* Ручная оплата: тапабельная ссылка на завершение оплаты */}
            {manualUrl && (
                <div className="checkout-floating-msg success">
                    <span>
                        {t('payment.manualInvoice')}{' '}
                        <a href={manualUrl} target="_blank" rel="noopener noreferrer">{manualUrl}</a>
                    </span>
                    <button type="button" onClick={() => setManualUrl(null)}>X</button>
                </div>
            )}

            {!token && (
                <div className="empty-state">
                    <div className="empty-icon">AU</div>
                    <h3>Требуется авторизация</h3>
                    <p>{error || 'Откройте Mini App повторно из бота, чтобы открыть магазин.'}</p>
                </div>
            )}

            {token && categories.length > 0 && (
                <div className="cat-tabs">
                    {categories.map((c) => (
                        <button
                            key={c.id}
                            className={`cat-tab ${activeCat === c.id ? 'active' : ''}`}
                            onClick={() => void loadProducts(c.id)}
                        >
                            {c.name}
                        </button>
                    ))}
                </div>
            )}

            {token && (categories.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">ST</div>
                    <h3>Магазин пуст</h3>
                    <p>Пока нет доступных товаров.</p>
                </div>
            ) : products.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">CT</div>
                    <h3>В категории пусто</h3>
                    <p>В выбранной категории пока нет товаров.</p>
                </div>
            ) : (
                <div className="product-grid">
                    {products.map((p) => (
                        <div key={p.id} className="product-card glass-card">
                            <div className="product-type-badge">{p.product_type.toUpperCase()}</div>
                            <h3 className="product-name">{p.name}</h3>
                            {p.description && <p className="product-desc">{p.description}</p>}
                            <div className="product-footer">
                                <span className="product-price">${p.price.toFixed(2)}</span>
                                <button
                                    className={`btn-add ${addedId === p.id ? 'added' : ''}`}
                                    onClick={() => addToCart(p.id)}
                                >
                                    {addedId === p.id ? 'Добавлено' : 'Добавить'}
                                </button>
                            </div>
                        </div>
                    ))}
                </div>
            ))}

            <DrawerModal
                open={token ? showCart : false}
                onClose={() => setShowCart(false)}
                title="Корзина"
                subtitle="Проверьте состав перед оплатой"
                footer={
                    cart.length > 0
                        ? <button className="btn-primary checkout-btn" onClick={checkout}>Выбрать способ оплаты</button>
                        : undefined
                }
            >
                {cart.length === 0 ? (
                    <p className="cart-empty">Корзина пока пустая.</p>
                ) : (
                    <>
                        <div className="cart-items">
                            {cart.map((item) => (
                                <div key={item.id} className="cart-item">
                                    <div>
                                        <span className="cart-item-name">{item.product_name}</span>
                                        <span className="cart-item-qty">x{item.quantity}</span>
                                    </div>
                                    <span className="cart-item-price">${item.total.toFixed(2)}</span>
                                </div>
                            ))}
                        </div>
                        <div className="cart-total">
                            <span>Итого</span>
                            <span className="cart-total-price">${cartTotal.toFixed(2)}</span>
                        </div>
                    </>
                )}
            </DrawerModal>

            <DrawerModal
                open={showPayModal && pendingOrderId !== null}
                onClose={() => setShowPayModal(false)}
                title={`Оплата заказа #${pendingOrderId ?? ''}`}
                subtitle="Выберите провайдера для генерации счета"
                footer={<button className="btn-ghost" onClick={() => setShowPayModal(false)}>Отмена</button>}
            >
                <div className="provider-list provider-card-list">
                    {providerCards.map((p) => (
                        <button
                            key={p.id}
                            className={`provider-btn provider-card ${p.accent}`}
                            onClick={() => handleProviderSelect(p.id)}
                        >
                            <span className="provider-card-copy">
                                <strong>{p.title}</strong>
                                <small>{p.description}</small>
                            </span>
                            <span className="provider-card-meta">
                                {p.badge && <span className="provider-pill">{p.badge}</span>}
                                <span className="provider-arrow">{'>'}</span>
                            </span>
                        </button>
                    ))}
                </div>
            </DrawerModal>

            {checkoutMsg && !showCart && (
                <div className={`checkout-floating-msg ${checkoutMsg.toLowerCase().includes('error') || checkoutMsg.toLowerCase().includes('failed') ? 'error' : 'success'}`}>
                    {checkoutMsg}
                    <button onClick={() => setCheckoutMsg('')}>X</button>
                </div>
            )}
        </div>
    )
}
