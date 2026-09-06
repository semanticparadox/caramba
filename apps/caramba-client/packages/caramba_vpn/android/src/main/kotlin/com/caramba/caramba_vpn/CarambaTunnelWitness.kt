package com.caramba.caramba_vpn

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import java.io.File
import java.net.Inet4Address
import java.net.NetworkInterface

// Параметры локального TUN-интерфейса.
//
// Вынесены из CarambaVpnService сюда, потому что теперь их читают ДВОЕ: сервис
// строит по ним интерфейс, а свидетель по ним же этот интерфейс узнаёт. Две
// копии одного адреса разошлись бы молча — и свидетель перестал бы находить
// живой туннель, то есть начал бы врать в самую опасную сторону.
internal object CarambaTun {
    const val ADDRESS = "172.19.0.1"
    const val PREFIX = 30
    const val MTU = 1500
    const val DNS = "1.1.1.1"
    const val DNS_FALLBACK = "8.8.8.8"
}

// Причины отказа, которые называет САМ КЛИЕНТ, а не ядро.
//
// Ядро присылает свои ошибки текстом, и приложение их переводит. Эти две
// придумывает клиент, поэтому они машинные и стабильные: их разбирает Dart
// (VpnFailureReason в lib/src/contract.dart) и подставляет русский текст. Один
// и тот же литерал по обе стороны провода — сверяется тестом.
internal object CarambaFailureReason {
    const val NO_VPN_TRANSPORT = "no_vpn_transport"
    const val TUN_NOT_SERVICED = "tun_adapter_not_serviced"
}

/**
 * Свидетель туннеля: ответ на вопрос «есть ли сейчас VPN» из источника,
 * который приложению не подчиняется.
 *
 * ЗАЧЕМ. Стадия `connected` до сих пор целиком принадлежала Go-ядру: оно само
 * себя объявляло подключённым, а приложение это повторяло. Когда ядро ошибалось
 * — а оно ошибалось (`executor.Shutdown()` глобален на процесс, и закрытие
 * служебного ядра валило чужой живой туннель, оставляя движок в
 * StateConnected), — проверить его было нечем: весь путь от mihomo до слова
 * «Защищено» состоял из утверждений одного и того же источника. Здесь появляется
 * второй, независимый: система.
 *
 * ПРАВИЛО ЧТЕНИЯ, БЕЗ КОТОРОГО СТАНОВИТСЯ ХУЖЕ. Ошибка в другую сторону —
 * «Отключено» на живом туннеле — опаснее исходного дефекта: человек, которому
 * сказали, что защиты нет, полезет её включать заново, оборвёт работающий
 * туннель и в худшем случае откроет трафик. Поэтому [Verdict.ABSENT] возвращается
 * ТОЛЬКО когда оба независимых наблюдения ответили «нет» и оба ответили без
 * ошибки. Молчание, отказ в разрешении, исключение, невиданный API — всё это
 * [Verdict.UNKNOWN], и вето по нему не срабатывает.
 *
 * ПОЧЕМУ НАБЛЮДЕНИЙ ДВА.
 *
 *  * `ConnectivityManager` — то же самое, чем определяется значок VPN в
 *    статус-баре. Но смотреть на АКТИВНУЮ сеть нельзя: сервис исключает своё
 *    приложение из туннеля (`addDisallowedApplication`), и для нашего же
 *    процесса активной сетью остаётся Wi-Fi, а не VPN. Живой туннель ответил бы
 *    «VPN нет» — ровно та ошибка, которой здесь быть не должно. Поэтому
 *    перебираются ВСЕ сети, а видит ли их приложение на этом устройстве вообще —
 *    вопрос, на который честный ответ иногда «не знаю».
 *  * Локальный TUN-интерфейс с нашим адресом. Он создаётся `establish()` и живёт,
 *    пока открыт дескриптор, — то есть не зависит ни от правил видимости сетей,
 *    ни от разрешений. Это якорь: пока он есть, вето не сработает никогда.
 */
internal object CarambaTunnelWitness {

    enum class Verdict(val wire: String) {
        /** Туннель наблюдается. */
        PRESENT("present"),

        /** Оба наблюдения ответили «нет», и оба ответили. Единственный повод для вето. */
        ABSENT("absent"),

        /** Спросить не удалось. НЕ повод для вето. */
        UNKNOWN("unknown"),
    }

    // Кадры идут раз в секунду, и каждый вызывает getifaddrs плюс обход сетей на
    // главном потоке. Короткий кэш убирает повтор внутри одного кадра (сток
    // события и ответ на `status` могут прийти подряд), но не настолько длинный,
    // чтобы вето отстало от действительности.
    private const val CACHE_TTL_MS = 700L

    @Volatile
    private var cachedAt = 0L

    @Volatile
    private var cached = Verdict.UNKNOWN

    /** Наблюдение с поправкой на короткий кэш. */
    @Synchronized
    fun verdict(ctx: Context): Verdict {
        val now = System.currentTimeMillis()
        // Часы могли прыгнуть назад (смена часового пояса, синхронизация
        // времени): отрицательный возраст кэша значил бы «свежий навсегда».
        val age = now - cachedAt
        if (age in 0 until CACHE_TTL_MS) return cached
        val v = observe(ctx)
        cached = v
        cachedAt = now
        return v
    }

    /** Сбросить кэш: подъём и разбор туннеля меняют ответ немедленно. */
    fun invalidate() {
        cachedAt = 0L
    }

    private fun observe(ctx: Context): Verdict {
        val transport = vpnTransport(ctx)
        if (transport == Verdict.PRESENT) return Verdict.PRESENT
        val iface = tunInterface()
        if (iface == Verdict.PRESENT) return Verdict.PRESENT
        // Утверждать отсутствие вправе только два состоявшихся отрицания.
        if (transport == Verdict.ABSENT && iface == Verdict.ABSENT) return Verdict.ABSENT
        return Verdict.UNKNOWN
    }

    /**
     * Есть ли в системе сеть с транспортом VPN.
     *
     * Требует ACCESS_NETWORK_STATE (обычное разрешение, у пользователя ничего не
     * спрашивается). Без него `getSystemService` ещё ответит, а вот
     * `getNetworkCapabilities` начнёт отдавать null — и перебор, не увидевший НИ
     * ОДНОЙ характеристики, обязан признаться, что не знает, вместо того чтобы
     * прочитать пустоту как «VPN нет».
     */
    private fun vpnTransport(ctx: Context): Verdict {
        return try {
            val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return Verdict.UNKNOWN
            @Suppress("DEPRECATION")
            val networks = cm.allNetworks
            var answered = false
            for (n in networks) {
                val caps = try {
                    cm.getNetworkCapabilities(n)
                } catch (_: Throwable) {
                    null
                } ?: continue
                answered = true
                if (caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) return Verdict.PRESENT
            }
            if (answered) Verdict.ABSENT else Verdict.UNKNOWN
        } catch (_: Throwable) {
            Verdict.UNKNOWN
        }
    }

    /** Живёт ли на каком-нибудь интерфейсе адрес нашего TUN. */
    private fun tunInterface(): Verdict {
        return try {
            val ifaces = NetworkInterface.getNetworkInterfaces() ?: return Verdict.UNKNOWN
            var answered = false
            while (ifaces.hasMoreElements()) {
                val ni = ifaces.nextElement() ?: continue
                answered = true
                if (carriesTunAddress(ni)) return Verdict.PRESENT
            }
            // Пустой перечень — это не «интерфейсов нет» (петля есть всегда), а
            // «перечислить не дали».
            if (answered) Verdict.ABSENT else Verdict.UNKNOWN
        } catch (_: Throwable) {
            Verdict.UNKNOWN
        }
    }

    private fun carriesTunAddress(ni: NetworkInterface): Boolean {
        val addrs = ni.inetAddresses ?: return false
        while (addrs.hasMoreElements()) {
            val a = addrs.nextElement() ?: continue
            if (a is Inet4Address && a.hostAddress == CarambaTun.ADDRESS) return true
        }
        return false
    }

    /** Имя интерфейса, несущего адрес TUN; null — не нашли или не дали посмотреть. */
    fun tunInterfaceName(): String? {
        return try {
            val ifaces = NetworkInterface.getNetworkInterfaces() ?: return null
            while (ifaces.hasMoreElements()) {
                val ni = ifaces.nextElement() ?: continue
                if (carriesTunAddress(ni)) return ni.name
            }
            null
        } catch (_: Throwable) {
            null
        }
    }
}

/**
 * Наблюдение за тем, ЧИТАЕТ ли кто-нибудь поднятый TUN-адаптер.
 *
 * ЗАЧЕМ ОТДЕЛЬНО ОТ СВИДЕТЕЛЯ. Проверка на устройстве поймала второй, более
 * тихий вид поломки: туннель поднялся, интерфейс создан (tun3), система честно
 * гонит в него весь трафик — а mihomo к дескриптору так и не подключился (в
 * журнале нет «[TUN] Tun adapter listening»). Свидетель в этот момент видит и
 * VPN-сеть, и адрес TUN, и говорит PRESENT — совершенно справедливо: туннель
 * есть. Только он никуда не ведёт. Сеть на устройстве при этом мертва целиком:
 * весь трафик уходит в интерфейс, из которого никто не читает.
 *
 * ПРИЗНАК. У TUN-устройства «tx» со стороны ядра — это пакеты, отданные на
 * чтение в user space, а «rx» — то, что user space записал обратно. Значит:
 *
 *  * `tx_dropped` растёт ТОЛЬКО когда очередь на чтение переполнена, то есть
 *    читателя нет. Работающий mihomo, даже не сумевший достучаться до узла,
 *    пакеты вычитывает, и этот счётчик стоит.
 *  * `rx_bytes > 0` означает, что ответ хоть раз вернулся: адаптер жив, и
 *    наблюдение навсегда снимается.
 *
 * Оба условия требуются вместе, и оба — на протяжении окна, а не мгновенно:
 * сразу после подъёма нулевой rx законен.
 *
 * Любая нечитаемость (`/sys` закрыт политикой, интерфейс не найден) — это
 * молчание, а не приговор: метод возвращает false и не делает ничего.
 */
internal class CarambaTunWatch {

    private companion object {
        /** Сколько ждать после подъёма, прежде чем вообще судить. */
        const val GRACE_MS = 10_000L

        /** На сколько должен вырасти счётчик отброшенных, чтобы это был не шум. */
        const val DROPPED_DELTA = 32L
    }

    private var iface: String? = null
    private var baselineAt = 0L
    private var baselineDropped = -1L

    /** Адаптер хоть раз ответил — судить больше не о чем. */
    private var alive = false

    /**
     * Один замер. `true` — адаптер поднят и НЕ обслуживается; во всех остальных
     * случаях (здоров, рано, нечитаемо, интерфейса нет) `false`.
     */
    fun unserviced(): Boolean {
        if (alive) return false
        val name = iface ?: CarambaTunnelWitness.tunInterfaceName()?.also { iface = it } ?: return false
        val rx = counter(name, "rx_bytes") ?: return false
        if (rx > 0) {
            alive = true
            return false
        }
        val dropped = counter(name, "tx_dropped") ?: return false
        val now = System.currentTimeMillis()
        if (baselineDropped < 0) {
            baselineDropped = dropped
            baselineAt = now
            return false
        }
        if (now - baselineAt < GRACE_MS) return false
        return dropped - baselineDropped >= DROPPED_DELTA
    }

    private fun counter(iface: String, name: String): Long? {
        return try {
            val f = File("/sys/class/net/$iface/statistics/$name")
            if (!f.canRead()) return null
            f.readText().trim().toLongOrNull()
        } catch (_: Throwable) {
            null
        }
    }
}
