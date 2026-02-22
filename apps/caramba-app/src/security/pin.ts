const PIN_STORAGE_KEY = 'caramba_miniapp_pin_v1';
const PIN_SESSION_UNLOCK_KEY = 'caramba_miniapp_pin_unlocked';

type StoredPin = {
    salt: string;
    hash: string;
    updated_at: number;
};

function toHex(bytes: Uint8Array): string {
    return Array.from(bytes)
        .map((b) => b.toString(16).padStart(2, '0'))
        .join('');
}

function fromHex(hex: string): Uint8Array {
    const clean = hex.trim().toLowerCase();
    const bytes = new Uint8Array(clean.length / 2);
    for (let i = 0; i < clean.length; i += 2) {
        bytes[i / 2] = parseInt(clean.slice(i, i + 2), 16);
    }
    return bytes;
}

function fallbackHash(input: string): string {
    // FNV-1a fallback for environments without SubtleCrypto.
    let hash = 0x811c9dc5;
    for (let i = 0; i < input.length; i++) {
        hash ^= input.charCodeAt(i);
        hash = (hash * 0x01000193) >>> 0;
    }
    return hash.toString(16).padStart(8, '0');
}

async function sha256(input: string): Promise<string> {
    if (window.crypto?.subtle) {
        const data = new TextEncoder().encode(input);
        const digest = await window.crypto.subtle.digest('SHA-256', data);
        return toHex(new Uint8Array(digest));
    }
    return fallbackHash(input);
}

function randomSaltHex(length = 16): string {
    if (window.crypto?.getRandomValues) {
        const bytes = new Uint8Array(length);
        window.crypto.getRandomValues(bytes);
        return toHex(bytes);
    }
    const random = `${Date.now()}-${Math.random()}-${Math.random()}`;
    return fallbackHash(random).padEnd(length * 2, '0').slice(0, length * 2);
}

function loadStoredPin(): StoredPin | null {
    const raw = localStorage.getItem(PIN_STORAGE_KEY);
    if (!raw) return null;
    try {
        const parsed = JSON.parse(raw) as StoredPin;
        if (!parsed?.salt || !parsed?.hash) return null;
        return parsed;
    } catch {
        return null;
    }
}

function saveStoredPin(pin: StoredPin): void {
    localStorage.setItem(PIN_STORAGE_KEY, JSON.stringify(pin));
}

export function hasPinConfigured(): boolean {
    return !!loadStoredPin();
}

export function isPinSessionUnlocked(): boolean {
    return sessionStorage.getItem(PIN_SESSION_UNLOCK_KEY) === '1';
}

export function markPinSessionUnlocked(unlocked: boolean): void {
    if (unlocked) {
        sessionStorage.setItem(PIN_SESSION_UNLOCK_KEY, '1');
    } else {
        sessionStorage.removeItem(PIN_SESSION_UNLOCK_KEY);
    }
}

export function validatePinFormat(pin: string): boolean {
    return /^\d{4}$/.test(pin);
}

export async function setPin(pin: string): Promise<void> {
    if (!validatePinFormat(pin)) {
        throw new Error('PIN must be exactly 4 digits.');
    }
    const salt = randomSaltHex(16);
    const hash = await sha256(`${salt}:${pin}`);
    saveStoredPin({ salt, hash, updated_at: Date.now() });
    markPinSessionUnlocked(true);
}

export async function verifyPin(pin: string): Promise<boolean> {
    const stored = loadStoredPin();
    if (!stored) return false;
    if (!validatePinFormat(pin)) return false;
    const hash = await sha256(`${stored.salt}:${pin}`);
    return hash === stored.hash;
}

export async function changePin(currentPin: string, newPin: string): Promise<void> {
    const ok = await verifyPin(currentPin);
    if (!ok) throw new Error('Current PIN is incorrect.');
    await setPin(newPin);
}

export async function disablePin(currentPin: string): Promise<void> {
    const ok = await verifyPin(currentPin);
    if (!ok) throw new Error('Current PIN is incorrect.');
    clearPin();
}

export function clearPin(): void {
    localStorage.removeItem(PIN_STORAGE_KEY);
    markPinSessionUnlocked(false);
}

export function getPinStorageMeta(): { updatedAt: number | null } {
    const stored = loadStoredPin();
    return { updatedAt: stored?.updated_at ?? null };
}

export function normalizePinInput(raw: string): string {
    return raw.replace(/\D/g, '').slice(0, 4);
}

export function isHexString(value: string): boolean {
    const normalized = value.trim().toLowerCase();
    if (normalized.length === 0 || normalized.length % 2 !== 0) return false;
    try {
        fromHex(normalized);
        return true;
    } catch {
        return false;
    }
}
