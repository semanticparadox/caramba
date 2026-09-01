import { QRCodeSVG } from 'qrcode.react'
import { useTranslation } from 'react-i18next'
import { copyText } from '../../lib/copyActions'
import { hapticSuccess } from '../../lib/haptics'
import { ExaIcon } from '../icons'
import { Button, Sheet } from '../ui'
import { useToast } from '../lib/useToast'

/** QR для любой ссылки: подписки целиком или конкретного инбаунда для роутера.
 *  Всегда белое поле — камеры читают его увереннее тёмного. */
export default function QrSheet({
    open,
    value,
    title,
    subtitle,
    onClose,
}: {
    open: boolean
    value: string
    title: string
    subtitle?: string
    onClose: () => void
}) {
    const { t } = useTranslation()
    const toast = useToast()
    return (
        <Sheet open={open} title={title} subtitle={subtitle} onClose={onClose}>
            <div className="exa-qr">
                <QRCodeSVG value={value} size={232} bgColor="#ffffff" fgColor="#0e1013" level="M" />
            </div>
            <Button
                variant="secondary"
                size="md"
                icon={<ExaIcon name="copy" size={20} />}
                onClick={async () => {
                    if (await copyText(value)) {
                        hapticSuccess()
                        toast(t('exa.common.copied'))
                    }
                }}
            >
                {t('exa.common.copyLink')}
            </Button>
        </Sheet>
    )
}
