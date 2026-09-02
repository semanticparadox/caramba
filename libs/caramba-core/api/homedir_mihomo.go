//go:build mihomo

package api

import mihomoconst "github.com/metacubex/mihomo/constant"

// setCoreHomeDir указывает ядру каталог, в котором оно ищет и докачивает
// geo-базы (geoip.metadb, GeoSite.dat) для правил GEOIP/GEOSITE.
//
// Без этого вызова на чистой машине ядро считает домашним каталогом текущий
// рабочий каталог процесса (для мобильного приложения он недоступен на запись),
// и разбор конфига падает на первом же правиле GEOIP: constant.Path.MMDB()
// читает каталог через os.ReadDir и при ошибке отдаёт пустой путь, после чего
// докачка валится с «can't download MMDB: open : no such file or directory».
//
// Каталог обязан существовать к моменту вызова — NewCore создаёт его (0700)
// раньше.
func setCoreHomeDir(dir string) { mihomoconst.SetHomeDir(dir) }
