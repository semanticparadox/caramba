/** Название страны по ISO-коду средствами Intl — без таблиц и без эмодзи-флагов.
 *  Если браузер не знает код, показываем сам код. */
export function countryName(code: string, locale: string): string {
    const cc = (code || '').toUpperCase()
    if (cc.length !== 2) return cc
    try {
        const dn = new Intl.DisplayNames([locale], { type: 'region' })
        return dn.of(cc) ?? cc
    } catch {
        return cc
    }
}
