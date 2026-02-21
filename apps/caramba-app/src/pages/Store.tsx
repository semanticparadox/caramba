import { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '../context/AuthContext';
import './Store.css';

interface Category {
    id: number;
    name: string;
    description: string | null;
}

interface Product {
    id: number;
    name: string;
    description: string | null;
    price: number;
    price_raw: number;
    product_type: string;
}

interface CartItem {
    id: number;
    product_id: number;
    product_name: string;
    quantity: number;
    price: number;
    total: number;
}

export default function Store() {
    const navigate = useNavigate();
    const { token, error } = useAuth();
    const [categories, setCategories] = useState<Category[]>([]);
    const [products, setProducts] = useState<Product[]>([]);
    const [cart, setCart] = useState<CartItem[]>([]);
    const [activeCat, setActiveCat] = useState<number | null>(null);
    const [loading, setLoading] = useState(true);
    const [showCart, setShowCart] = useState(false);
    const [checkoutMsg, setCheckoutMsg] = useState('');
    const [addedId, setAddedId] = useState<number | null>(null);

    const headers = { Authorization: `Bearer ${token}` };

    useEffect(() => {
        if (!token) {
            setLoading(false);
            return;
        }
        fetch('/api/client/store/categories', { headers })
            .then(r => r.json())
            .then(data => {
                setCategories(data);
                if (data.length > 0) {
                    setActiveCat(data[0].id);
                    loadProducts(data[0].id);
                }
            })
            .catch(console.error)
            .finally(() => setLoading(false));
    }, [token]);

    const goBack = () => {
        if (window.history.length > 1) {
            navigate(-1);
        } else {
            navigate('/');
        }
    };

    const loadProducts = async (catId: number) => {
        setActiveCat(catId);
        try {
            const res = await fetch(`/api/client/store/products/${catId}`, { headers });
            if (res.ok) setProducts(await res.json());
        } catch (e) { console.error(e); }
    };

    const loadCart = async () => {
        try {
            const res = await fetch('/api/client/store/cart', { headers });
            if (res.ok) setCart(await res.json());
        } catch (e) { console.error(e); }
    };

    const addToCart = async (productId: number) => {
        try {
            const res = await fetch('/api/client/store/cart/add', {
                method: 'POST',
                headers: { ...headers, 'Content-Type': 'application/json' },
                body: JSON.stringify({ product_id: productId, quantity: 1 }),
            });
            if (res.ok) {
                setAddedId(productId);
                setTimeout(() => setAddedId(null), 1500);
                await loadCart();
            }
        } catch (e) { console.error(e); }
    };

    const checkout = async () => {
        setCheckoutMsg('');
        try {
            const res = await fetch('/api/client/store/checkout', {
                method: 'POST',
                headers,
            });
            if (res.ok) {
                setCheckoutMsg('✅ Order placed successfully!');
                setCart([]);
                setTimeout(() => setCheckoutMsg(''), 3000);
            } else {
                const err = await res.text();
                setCheckoutMsg(`❌ ${err}`);
            }
        } catch (e) { setCheckoutMsg('❌ Network error'); }
    };

    const openCart = () => {
        loadCart();
        setShowCart(true);
    };

    const cartTotal = cart.reduce((s, i) => s + i.total, 0);

    if (loading) return <div className="page"><div className="loading">Loading store...</div></div>;

    return (
        <div className="page store-page">
            <header className="page-header">
                <button className="back-button" onClick={goBack}>←</button>
                <h2>📦 Store</h2>
                <button className="cart-toggle" onClick={openCart}>
                    🛒 {cart.length > 0 && <span className="cart-count">{cart.length}</span>}
                </button>
            </header>

            {!token && (
                <div className="empty-state">
                    <div className="empty-icon">🔐</div>
                    <h3>Authorization Required</h3>
                    <p>{error || 'Reopen Mini App from bot to access the store.'}</p>
                </div>
            )}

            {/* Category Tabs */}
            {token && categories.length > 0 && (
                <div className="cat-tabs">
                    {categories.map(c => (
                        <button
                            key={c.id}
                            className={`cat-tab ${activeCat === c.id ? 'active' : ''}`}
                            onClick={() => loadProducts(c.id)}
                        >
                            {c.name}
                        </button>
                    ))}
                </div>
            )}

            {/* Products */}
            {token && (categories.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">📦</div>
                    <h3>Store is empty</h3>
                    <p>No products available yet.</p>
                </div>
            ) : products.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-icon">🏷️</div>
                    <h3>No products</h3>
                    <p>This category has no products yet.</p>
                </div>
            ) : (
                <div className="product-grid">
                    {products.map(p => (
                        <div key={p.id} className="product-card glass-card">
                            <div className="product-type-badge">{p.product_type}</div>
                            <h3 className="product-name">{p.name}</h3>
                            {p.description && <p className="product-desc">{p.description}</p>}
                            <div className="product-footer">
                                <span className="product-price">${p.price.toFixed(2)}</span>
                                <button
                                    className={`btn-add ${addedId === p.id ? 'added' : ''}`}
                                    onClick={() => addToCart(p.id)}
                                >
                                    {addedId === p.id ? '✓ Added' : '+ Add'}
                                </button>
                            </div>
                        </div>
                    ))}
                </div>
            ))}

            {/* Cart Overlay */}
            {token && showCart && (
                <div className="cart-overlay" onClick={() => setShowCart(false)}>
                    <div className="cart-panel glass-card" onClick={e => e.stopPropagation()}>
                        <div className="cart-header">
                            <h3>🛒 Your Cart</h3>
                            <button className="close-btn" onClick={() => setShowCart(false)}>✕</button>
                        </div>

                        {cart.length === 0 ? (
                            <p className="cart-empty">Your cart is empty.</p>
                        ) : (
                            <>
                                <div className="cart-items">
                                    {cart.map(item => (
                                        <div key={item.id} className="cart-item">
                                            <div>
                                                <span className="cart-item-name">{item.product_name}</span>
                                                <span className="cart-item-qty">×{item.quantity}</span>
                                            </div>
                                            <span className="cart-item-price">${item.total.toFixed(2)}</span>
                                        </div>
                                    ))}
                                </div>
                                <div className="cart-total">
                                    <span>Total</span>
                                    <span className="cart-total-price">${cartTotal.toFixed(2)}</span>
                                </div>
                                <button className="btn-primary checkout-btn" onClick={checkout}>
                                    💳 Pay from Balance
                                </button>
                            </>
                        )}

                        {checkoutMsg && (
                            <div className={`checkout-msg ${checkoutMsg.startsWith('✅') ? 'success' : 'error'}`}>
                                {checkoutMsg}
                            </div>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
