//go:build mihomo

package main

import mihomoconst "github.com/metacubex/mihomo/constant"

// hasMihomoCore сообщает, что бинарник собран с нативным ядром и дымовой прогон
// имеет смысл.
const hasMihomoCore = true

// setCoreHomeDir указывает ядру каталог, в котором оно ищет и докачивает
// geo-базы (geoip.metadb, GeoSite.dat) для правил GEOIP/GEOSITE. Каталог должен
// существовать: constant.Path.MMDB() читает его через os.ReadDir и при ошибке
// отдаёт пустой путь, после чего разбор конфига падает на первом же правиле
// GEOIP.
func setCoreHomeDir(dir string) { mihomoconst.SetHomeDir(dir) }
