import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react'

const ToastContext = createContext<(text: string) => void>(() => {})

/** Короткое подтверждение вроде «Скопировано»: одна строка, 1.6 с, без кнопок. */
export function ToastProvider({ children }: { children: ReactNode }) {
    const [text, setText] = useState<string | null>(null)
    const timer = useRef<ReturnType<typeof setTimeout> | null>(null)

    const show = useCallback((value: string) => {
        setText(value)
        if (timer.current) clearTimeout(timer.current)
        timer.current = setTimeout(() => setText(null), 1600)
    }, [])

    useEffect(() => () => {
        if (timer.current) clearTimeout(timer.current)
    }, [])

    return (
        <ToastContext.Provider value={show}>
            {children}
            {text ? (
                <div className="exa-toast" role="status">
                    {text}
                </div>
            ) : null}
        </ToastContext.Provider>
    )
}

export function useToast() {
    return useContext(ToastContext)
}
