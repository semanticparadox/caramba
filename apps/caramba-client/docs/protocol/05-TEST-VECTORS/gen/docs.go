package main

// Payload builders, one per document type. Field numbers and caps are
// 03-WIRE.md section 8; the enumerations are section 5.

import "fmt"

// Fixture constants. The seeds and the derived values are 03-WIRE.md
// section 15; every one of them was checked against that section before this
// file was written, and main.go re-checks the five published frame digests.
const (
	fixIAT       = 1788307200 // 2026-09-02T00:00:00Z
	fixSubUUID   = "9f3c1d02-5b8e-4a17-9d44-0e7a6c11b3f8"
	fixGen       = 1
	fixCatalogV  = 7
	fixDirV      = 412
	fixTier      = 1
	fixTTL       = 7200
	fixJit       = 20
	fixNonceHex  = "a3f10c94b27e5d6188ff20419c73ae05"
	fixDtpLabel  = "csm1-doc-example-device-spki"
	fixLocSecret = "csm1-doc-example-loc-secret"
)

// envelope builds the common envelope, 03-WIRE.md section 8.0.
func envelope(pid []byte, ver, iat, life uint64) *cmap {
	return m().
		set(1, u(1)).
		set(2, b(pid)).
		set(3, u(ver)).
		set(4, u(iat)).
		set(5, u(iat+life))
}

// ---------------------------------------------------------------- 0x01 key doc

type keyEntry struct {
	kid []byte
	pk  []byte
}

func keyEntryVal(k keyEntry) val {
	return m().set(1, b(k.kid)).set(2, u(1)).set(3, b(k.pk))
}

type roleSpec struct {
	role uint64
	ks   [][]byte
	thr  uint64
}

type keyDocOpts struct {
	pid    []byte
	ver    uint64
	iat    uint64
	keys   []keyEntry
	roles  []roleSpec
	revKid [][]byte
	revNod []string
	tiers  map[uint64][]byte
	dep    []depEntry
	ttlk   uint64
}

type depEntry struct {
	surface string
	sunset  uint64
}

func buildKeyDoc(o keyDocOpts) *cmap {
	c := envelope(o.pid, o.ver, o.iat, lifeKey)
	ka := arr{}
	for _, k := range o.keys {
		ka = append(ka, keyEntryVal(k))
	}
	c.set(10, ka)
	rm := m()
	for _, r := range o.roles {
		ks := arr{}
		for _, k := range r.ks {
			ks = append(ks, b(k))
		}
		rm.set(r.role, m().set(1, ks).set(2, u(r.thr)))
	}
	c.set(11, rm)
	if len(o.revKid) > 0 || len(o.revNod) > 0 {
		rv := m()
		if len(o.revKid) > 0 {
			a := arr{}
			for _, k := range o.revKid {
				a = append(a, b(k))
			}
			rv.set(1, a)
		}
		if len(o.revNod) > 0 {
			a := arr{}
			for _, n := range o.revNod {
				a = append(a, t(n))
			}
			rv.set(2, a)
		}
		c.set(12, rv)
	}
	if len(o.tiers) > 0 {
		tm := m()
		for k, v := range o.tiers {
			tm.set(k, b(v))
		}
		c.set(13, tm)
	}
	if len(o.dep) > 0 {
		a := arr{}
		for _, d := range o.dep {
			a = append(a, m().set(1, t(d.surface)).set(2, u(d.sunset)))
		}
		c.set(15, a)
	}
	c.setIf(o.ttlk != 0, 16, u(o.ttlk))
	return c
}

// ---------------------------------------------------------------- node entry

// node mirrors the field table of 03-WIRE.md 8.2.1. A zero value means the
// key is omitted, which is what the "renderer MUST omit the key entirely"
// rule for fl = 0 requires.
type node struct {
	id   string
	pn   string
	cc   string
	h    string
	p    uint64
	pr   uint64
	nw   uint64
	se   uint64
	sni  string
	pbk  []byte
	sid  string
	fp   uint64
	fl   uint64
	pt   string
	hst  string
	alp  []uint64
	hop  string
	obf  string
	cg   uint64
	zr   bool
	ins  bool
	insV bool // emit the ins key even when false
	rl   string
	ssm  uint64
	mtu  uint64
}

func nodeVal(n node) val {
	c := m().
		set(1, t(n.id)).
		set(2, t(n.pn)).
		set(3, t(n.cc)).
		set(4, t(n.h)).
		set(5, u(n.p)).
		set(6, u(n.pr)).
		set(7, u(n.nw)).
		set(8, u(n.se))
	c.setIf(n.sni != "", 9, t(n.sni))
	c.setIf(len(n.pbk) > 0, 10, b(n.pbk))
	c.setIf(n.sid != "", 11, t(n.sid))
	c.setIf(n.fp != 0, 12, u(n.fp))
	c.setIf(n.fl != 0, 13, u(n.fl))
	c.setIf(n.pt != "", 14, t(n.pt))
	c.setIf(n.hst != "", 15, t(n.hst))
	if len(n.alp) > 0 {
		a := arr{}
		for _, x := range n.alp {
			a = append(a, u(x))
		}
		c.set(16, a)
	}
	c.setIf(n.hop != "", 17, t(n.hop))
	c.setIf(n.obf != "", 18, t(n.obf))
	c.setIf(n.cg != 0, 19, u(n.cg))
	c.setIf(n.zr, 20, boolean(true))
	c.setIf(n.ins || n.insV, 21, boolean(n.ins))
	c.setIf(n.rl != "", 22, t(n.rl))
	c.setIf(n.ssm != 0, 23, u(n.ssm))
	c.setIf(n.mtu != 0, 24, u(n.mtu))
	return c
}

// The five node shapes 03-WIRE.md 8.2.1 measures. realityPBK is the fixture
// Reality public key of the published catalog: bytes 0x00..0x1f.
var realityPBK = func() []byte {
	x := make([]byte, 32)
	for i := range x {
		x[i] = byte(i)
	}
	return x
}()

func nodeVlessReality(id, pn, cc, host string, port uint64, sid string) node {
	return node{
		id: id, pn: pn, cc: cc, h: host, p: port,
		pr: 1, nw: 1, se: 2,
		sni: "www.microsoft.com", pbk: realityPBK, sid: sid,
		fp: 1, fl: 1, insV: true,
	}
}

// ---------------------------------------------------------------- 0x02 catalog

type mirror struct {
	h   string
	sni string
	pin [][]byte
	asn uint64
	cc  string
	w   uint64
	ip  []string
}

func mirrorVal(x mirror) val {
	pins := arr{}
	for _, p := range x.pin {
		pins = append(pins, b(p))
	}
	c := m().set(1, t(x.h)).set(2, t(x.sni)).set(3, pins).set(4, u(x.asn)).set(5, t(x.cc))
	c.setIf(x.w != 0, 6, u(x.w))
	if len(x.ip) > 0 {
		a := arr{}
		for _, i := range x.ip {
			a = append(a, t(i))
		}
		c.set(7, a)
	}
	return c
}

type dohEntry struct {
	h   string
	p   string
	ip  []string
	pin [][]byte
}

func dohVal(x dohEntry) val {
	ips := arr{}
	for _, i := range x.ip {
		ips = append(ips, t(i))
	}
	pins := arr{}
	for _, p := range x.pin {
		pins = append(pins, b(p))
	}
	return m().set(1, t(x.h)).set(2, t(x.p)).set(3, ips).set(4, pins)
}

type resource struct {
	n  string
	u  string
	h  []byte
	iv uint64
}

func resourceVal(x resource) val {
	c := m().set(1, t(x.n)).set(2, t(x.u)).set(3, b(x.h))
	c.setIf(x.iv != 0, 4, u(x.iv))
	return c
}

type catalogOpts struct {
	pid   []byte
	ver   uint64
	iat   uint64
	tier  uint64
	ex    []node
	re    []node
	ro    []routeEntry
	cap   []byte
	mir   []mirror
	doh   []dohEntry
	rs    []resource
	geo   []resource
	ttl   uint64
	jit   uint64
	thr   [3]uint64 // conn_bytes, conn_packets, resp_max
	pb    [2]uint64
	lad   *ladder
	pin   []pinEntry
	hpk   []byte
	hpkv  uint64
	noThr bool
}

type routeEntry struct {
	id string
	nm string
	rs []string
}

type pinEntry struct {
	h    string
	spki [][]byte
}

type ladder struct {
	ord []uint64
	en  []uint64
}

func buildCatalog(o catalogOpts) *cmap {
	c := envelope(o.pid, o.ver, o.iat, lifeCatalog)
	c.set(10, u(o.tier))
	ex := arr{}
	for _, n := range o.ex {
		ex = append(ex, nodeVal(n))
	}
	c.set(11, ex)
	if len(o.re) > 0 {
		a := arr{}
		for _, n := range o.re {
			a = append(a, nodeVal(n))
		}
		c.set(12, a)
	}
	if len(o.ro) > 0 {
		a := arr{}
		for _, r := range o.ro {
			rs := arr{}
			for _, x := range r.rs {
				rs = append(rs, t(x))
			}
			a = append(a, m().set(1, t(r.id)).set(2, t(r.nm)).set(3, rs))
		}
		c.set(13, a)
	}
	c.set(14, b(o.cap))
	if len(o.mir) > 0 {
		a := arr{}
		for _, x := range o.mir {
			a = append(a, mirrorVal(x))
		}
		c.set(15, a)
	}
	if len(o.doh) > 0 {
		a := arr{}
		for _, x := range o.doh {
			a = append(a, dohVal(x))
		}
		c.set(16, a)
	}
	if len(o.rs) > 0 {
		a := arr{}
		for _, x := range o.rs {
			a = append(a, resourceVal(x))
		}
		c.set(17, a)
	}
	if len(o.geo) > 0 {
		a := arr{}
		for _, x := range o.geo {
			a = append(a, resourceVal(x))
		}
		c.set(18, a)
	}
	c.set(19, u(o.ttl))
	c.set(20, u(o.jit))
	if !o.noThr {
		c.set(21, m().set(1, u(o.thr[0])).set(2, u(o.thr[1])).set(3, u(o.thr[2])))
	}
	c.set(22, arr{u(o.pb[0]), u(o.pb[1])})
	if o.lad != nil {
		ord := arr{}
		for _, r := range o.lad.ord {
			ord = append(ord, u(r))
		}
		en := arr{}
		for _, r := range o.lad.en {
			en = append(en, u(r))
		}
		c.set(23, m().set(1, ord).set(2, en))
	}
	if len(o.pin) > 0 {
		a := arr{}
		for _, p := range o.pin {
			sp := arr{}
			for _, s := range p.spki {
				sp = append(sp, b(s))
			}
			a = append(a, m().set(1, t(p.h)).set(2, sp))
		}
		c.set(24, a)
	}
	if len(o.hpk) > 0 {
		c.set(25, b(o.hpk))
		c.set(26, u(o.hpkv))
	}
	return c
}

// ---------------------------------------------------------------- 0x03 directive

type selection struct {
	exit    string
	relay   string
	preset  string
	variant uint64
	proto   uint64
	rcc     string
	nid     uint64
	set     bool
}

type policyItem struct {
	key uint64
	v   val
	src uint64
}

type hint struct {
	k uint64
	s string
}

type traffic struct {
	up, dn, tot, exp uint64
	set              bool
}

type directiveOpts struct {
	pid   []byte
	ver   uint64
	iat   uint64
	nonce []byte
	dtp   []byte
	st    uint64
	rc    uint64
	cat   []byte
	cn    uint64
	tier  uint64
	cap   []byte
	sel   *selection
	pol   []policyItem
	ann   string
	sup   string
	ui    []hint
	ttl   uint64
	exph  uint64
	loc   string
	traf  *traffic
}

func buildDirective(o directiveOpts) *cmap {
	c := envelope(o.pid, o.ver, o.iat, lifeDirective)
	c.set(10, b(o.nonce))
	c.set(11, b(o.dtp))
	c.set(12, u(o.st))
	c.setIf(o.rc != 0, 13, u(o.rc))
	c.set(14, b(o.cat))
	c.set(15, u(o.cn))
	c.set(16, u(o.tier))
	c.set(17, b(o.cap))
	if o.sel != nil {
		s := m()
		s.setIf(o.sel.exit != "", 1, t(o.sel.exit))
		s.setIf(o.sel.relay != "", 2, t(o.sel.relay))
		s.setIf(o.sel.preset != "", 3, t(o.sel.preset))
		s.setIf(o.sel.variant != 0, 4, u(o.sel.variant))
		s.setIf(o.sel.proto != 0, 5, u(o.sel.proto))
		s.setIf(o.sel.rcc != "", 6, t(o.sel.rcc))
		s.setIf(o.sel.nid != 0, 7, u(o.sel.nid))
		c.set(18, s)
	}
	if len(o.pol) > 0 {
		p := m()
		for _, it := range o.pol {
			p.set(it.key, arr{it.v, u(it.src)})
		}
		c.set(19, p)
	}
	c.setIf(o.ann != "", 20, t(o.ann))
	c.setIf(o.sup != "", 21, t(o.sup))
	if len(o.ui) > 0 {
		a := arr{}
		for _, h := range o.ui {
			a = append(a, m().set(1, u(h.k)).set(2, t(h.s)))
		}
		c.set(22, a)
	}
	c.set(23, u(o.ttl))
	c.setIf(o.exph != 0, 24, u(o.exph))
	c.set(25, t(o.loc))
	if o.traf != nil {
		c.set(26, m().set(1, u(o.traf.up)).set(2, u(o.traf.dn)).set(3, u(o.traf.tot)).set(4, u(o.traf.exp)))
	}
	return c
}

// ---------------------------------------------------------------- 0x04 chunk

// chunkPayloads slices a complete catalog FRAME, not a catalog payload.
// 03-WIRE.md section 8.4.
func chunkPayloads(pid []byte, ver, iat uint64, catFrame []byte) []*cmap {
	tl := len(catFrame)
	n := (tl + chunkPayloadMax - 1) / chunkPayloadMax
	if n < 1 {
		n = 1
	}
	if n > 64 {
		panic(fmt.Sprintf("chunk: %d chunks exceeds the 1..64 cap", n))
	}
	cid := frameSHA(catFrame)[:10]
	out := make([]*cmap, 0, n)
	for i := 0; i < n; i++ {
		lo := i * chunkPayloadMax
		hi := lo + chunkPayloadMax
		if hi > tl {
			hi = tl
		}
		c := envelope(pid, ver, iat, lifeCatalog)
		c.set(10, b(cid))
		c.set(11, u(uint64(i)))
		c.set(12, u(uint64(n)))
		c.set(13, u(uint64(tl)))
		c.set(14, b(catFrame[lo:hi]))
		out = append(out, c)
	}
	return out
}

// ---------------------------------------------------------------- 0x05 blob

type blobOpts struct {
	pid  []byte
	ver  uint64
	iat  uint64
	org  string
	code string
	rk   []byte
	mir  []mirror
	doh  []dohEntry
	nm   string
}

func buildBlob(o blobOpts) *cmap {
	c := envelope(o.pid, o.ver, o.iat, lifeBootstrap)
	c.set(10, t(o.org))
	c.set(11, t(o.code))
	c.set(12, b(o.rk))
	ma := arr{}
	for _, x := range o.mir {
		ma = append(ma, mirrorVal(x))
	}
	c.set(13, ma)
	da := arr{}
	for _, x := range o.doh {
		da = append(da, dohVal(x))
	}
	c.set(14, da)
	c.setIf(o.nm != "", 15, t(o.nm))
	return c
}

// ---------------------------------------------------------------- 0x08 reserve

func buildReserve(pid []byte, ver, iat uint64, mirs []mirror, dohs []dohEntry, coh uint64) *cmap {
	c := envelope(pid, ver, iat, lifeReserve)
	ma := arr{}
	for _, x := range mirs {
		ma = append(ma, mirrorVal(x))
	}
	c.set(10, ma)
	if len(dohs) > 0 {
		da := arr{}
		for _, x := range dohs {
			da = append(da, dohVal(x))
		}
		c.set(11, da)
	}
	c.setIf(coh != 0, 12, u(coh))
	return c
}

// ---------------------------------------------------------------- 0x06 sealed

type sealedOpts struct {
	pid  []byte
	ver  uint64
	iat  uint64
	dtp  []byte
	kem  uint64
	kdf  uint64
	aead uint64
	enc  []byte
	ct   []byte
	rkv  uint64
}

func buildSealed(o sealedOpts) *cmap {
	c := envelope(o.pid, o.ver, o.iat, lifeDirective)
	c.set(10, b(o.dtp))
	c.set(11, u(o.kem))
	c.set(12, u(o.kdf))
	c.set(13, u(o.aead))
	c.set(14, b(o.enc))
	c.set(15, b(o.ct))
	c.set(16, u(o.rkv))
	return c
}
